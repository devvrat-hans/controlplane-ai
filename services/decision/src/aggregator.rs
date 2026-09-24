use std::collections::HashMap;

use uuid::Uuid;

use controlplane_common::models::{Decision, Verdict};
use controlplane_common::types::{AppId, Axis, Outcome};

/// Suffix marking a shadow verdict as a sub-threshold *evidence* reading rather than an
/// actionable finding. Must match `controlplane_shadow_analysis::laya_client::EVIDENCE_SUFFIX`;
/// the decision crate deliberately does not depend on the shadow crate (contract: the two
/// communicate over events, never a shared reference), so the literal is repeated here and
/// `evidence_suffix_matches_the_shadow_path_convention` pins it.
pub const EVIDENCE_SUFFIX: &str = "-evidence";

/// Check-name prefix used by the calibrated decision-model judge (Laya / Jev).
/// Keeps judge verdicts distinguishable from the in-process heuristics.
pub const JUDGE_DETECTOR_PREFIX: &str = "laya-";

/// Per-detector reliability weights plus the deterministic threshold map.
///
/// Fitted offline against `reviewer_overrides` and stored in `detector_calibration`
/// (migration 023). This struct is *only* constructed from that store — and only from
/// rows where `calibrated = TRUE` — so an un-fitted system falls back to
/// [`VerdictAggregator::aggregate_with_reasoning`] and behaves exactly as before.
#[derive(Debug, Clone)]
pub struct FusionConfig {
    /// Reliability weight per detector `check_name` (the `-evidence` suffix is stripped
    /// before lookup, so an evidence reading uses its detector's weight).
    ///
    /// A detector **absent** from this map is not fused at all: its verdict keeps exactly
    /// the authority it has today. That is deliberate — a fit that covers only some
    /// detectors must not silently down-weight the rest.
    pub weights: HashMap<String, f64>,
    /// p_axis >= this escalates (or blocks, if the axis policy allows).
    pub escalate_threshold: f64,
    /// p_axis >= this edits.
    pub edit_threshold: f64,
    /// Below this a reading is discarded rather than fused.
    pub evidence_threshold: f64,
    /// A detector must reach this to count as one half of a corroboration pair.
    pub corroboration_floor: f64,
    /// A calibrated judge at or above this can act without corroboration.
    pub calibrated_judge_confidence: f64,
    /// How far the judge and the heuristics must be apart (in probability) on the same
    /// axis before the conflict is routed to a human.
    pub disagreement_delta: f64,
    /// Version of the fit behind these weights, recorded on the decision for auditing.
    pub calibration_version: Option<i32>,
}

impl Default for FusionConfig {
    fn default() -> Self {
        Self {
            weights: HashMap::new(),
            escalate_threshold: 0.90,
            edit_threshold: 0.70,
            evidence_threshold: 0.45,
            corroboration_floor: 0.5,
            calibrated_judge_confidence: 0.9,
            disagreement_delta: 0.4,
            calibration_version: None,
        }
    }
}

impl FusionConfig {
    /// The fusion is opt-in: with no fitted detector weights there are no "calibrated
    /// inputs", so the caller must keep using the pre-existing aggregator.
    pub fn is_enabled(&self) -> bool {
        !self.weights.is_empty()
    }

    /// Fitted reliability weight for a detector, or `None` when it is outside the fit.
    fn fitted_weight(&self, check_name: &str) -> Option<f64> {
        self.weights
            .get(base_detector(check_name))
            .copied()
            .map(|weight| weight.clamp(0.0, 1.0))
    }
}

/// Raise an outcome to human review without ever *lowering* a stronger action.
///
/// `Outcome::worst` cannot be used for "at least Escalate": this codebase orders
/// `Pass < Escalate < Edit < Block`, so `worst(Edit, Escalate)` is `Edit`. The existing
/// aggregator already treats Escalate as the human-review escalation *of* an automatic
/// edit (see its compound-risk rule), and this mirrors that convention exactly.
fn escalate_at_least(outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::Pass | Outcome::Edit => Outcome::Escalate,
        Outcome::Escalate => Outcome::Escalate,
        // A block already outranks review; never weaken it.
        Outcome::Block => Outcome::Block,
    }
}

/// Strip the evidence suffix so an evidence reading is attributed to its detector.
pub fn base_detector(check_name: &str) -> &str {
    check_name
        .strip_suffix(EVIDENCE_SUFFIX)
        .unwrap_or(check_name)
}

/// True when a check is the calibrated judge rather than an in-process heuristic.
pub fn is_judge_detector(check_name: &str) -> bool {
    base_detector(check_name).starts_with(JUDGE_DETECTOR_PREFIX)
}

/// One detector's contribution to an axis score.
#[derive(Debug, Clone)]
pub struct DetectorContribution {
    pub check_name: String,
    /// The detector's own outcome; `Pass` means it was an evidence-only reading.
    pub outcome: Outcome,
    /// Calibrated probability the detector reported.
    pub p: f32,
    /// Weight applied in the noisy-OR, or `None` when the detector is outside the fit
    /// and its verdict was passed through with its existing authority.
    pub weight: Option<f32>,
    /// True for the calibrated judge, false for a heuristic or a specialist model.
    pub calibrated: bool,
    pub verdict_id: Uuid,
}

/// Fused score for a single axis.
#[derive(Debug, Clone)]
pub struct AxisFusion {
    pub axis: Axis,
    pub p: f32,
    pub outcome: Outcome,
    pub detectors: Vec<DetectorContribution>,
}

/// The result of the fusion stage: the aggregation plus the explainability a reviewer
/// needs to see *why* the calibrated decision differs from the raw one.
#[derive(Debug, Clone)]
pub struct FusionResult {
    pub aggregation: AggregationResult,
    pub axes: Vec<AxisFusion>,
    /// True when the judge and the heuristics materially disagreed somewhere.
    pub disagreement: bool,
    pub disagreement_reason: Option<String>,
    pub calibration_version: Option<i32>,
}

/// Aggregates verdicts from fast-path and shadow-path,
/// applies confidence weighting, produces final decision.
pub struct VerdictAggregator;

impl VerdictAggregator {
    pub fn new() -> Self {
        Self
    }

    /// Aggregate a set of verdicts into a final Decision.
    ///
    /// Logic:
    /// - Final outcome = worst verdict (block > edit > escalate > pass)
    /// - If conflicting verdicts at the same severity, the one with higher confidence wins
    /// - Contributing verdicts are all non-pass verdicts
    pub fn aggregate(
        &self,
        call_id: Uuid,
        app_id: AppId,
        verdicts: &[Verdict],
        policy_version: Option<i32>,
    ) -> Decision {
        Decision::from_verdicts(call_id, app_id, verdicts, policy_version)
    }

    /// Aggregate with confidence-weighted tie-breaking and intersection escalation.
    /// Returns (final_outcome, primary_reason, contributing_verdict_ids).
    ///
    /// Intersection escalation: when 2+ different axes produce non-pass verdicts,
    /// escalate even if individual confidences are below threshold. This handles
    /// overlapping risks (e.g., hallucination + privacy = compound risk).
    pub fn aggregate_with_reasoning(
        &self,
        verdicts: &[Verdict],
    ) -> AggregationResult {
        if verdicts.is_empty() {
            return AggregationResult {
                final_outcome: Outcome::Pass,
                primary_reason: "No verdicts received".to_string(),
                contributing_ids: Vec::new(),
                confidence: 1.0,
                compound_risk: false,
                triggered_axes: Vec::new(),
            };
        }

        let worst = verdicts.iter()
            .map(|v| v.outcome)
            .fold(Outcome::Pass, Outcome::worst);

        // Collect unique axes that produced non-pass verdicts
        let mut triggered_axes: Vec<String> = verdicts.iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .map(|v| v.axis.as_str().to_string())
            .collect();
        triggered_axes.sort();
        triggered_axes.dedup();

        let compound_risk = triggered_axes.len() >= 2;

        // Intersection escalation: if 2+ axes fire, escalate even if individual
        // outcomes were just "edit" or low-confidence "escalate"
        let final_outcome = if compound_risk && worst == Outcome::Pass {
            Outcome::Escalate
        } else if compound_risk && worst == Outcome::Edit {
            // Compound risk upgrades "edit" to "escalate"
            Outcome::Escalate
        } else {
            worst
        };

        // Find the verdict(s) at the worst level with highest confidence
        let contributing: Vec<&Verdict> = verdicts.iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .collect();
        let worst_level: Vec<&Verdict> = verdicts.iter()
            .filter(|v| v.outcome == final_outcome)
            .collect();
        // Fall back to all contributing verdicts when the final outcome came from
        // intersection escalation rather than any single verdict's level.
        let pool: Vec<&Verdict> = if worst_level.is_empty() { contributing.clone() } else { worst_level };

        let primary = if pool.is_empty() {
            verdicts.first().unwrap()
        } else {
            pool.iter()
                .max_by(|a, b| a.confidence.partial_cmp(&b.confidence).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap()
        };

        let primary_reason = if compound_risk {
            format!(
                "Compound risk: {} axes triggered ({}). {}",
                triggered_axes.len(),
                triggered_axes.join(", "),
                primary.reason
            )
        } else {
            primary.reason.clone()
        };

        let all_non_pass: Vec<Uuid> = verdicts.iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .map(|v| v.id)
            .collect();

        AggregationResult {
            final_outcome,
            primary_reason,
            contributing_ids: all_non_pass,
            confidence: primary.confidence,
            compound_risk,
            triggered_axes,
        }
    }
}

impl VerdictAggregator {
    /// Fuse calibrated detector scores into a final outcome (plan §5.2).
    ///
    /// Deterministic by construction: a pure function of the verdicts' calibrated
    /// probabilities and the stored weights/thresholds. No model runs here — the models
    /// supplied scores upstream; this applies policy.
    ///
    /// Returns `None` when the fusion is not configured, which is the signal for the
    /// caller to use [`VerdictAggregator::aggregate_with_reasoning`] instead. That is
    /// what keeps an un-fitted deployment byte-for-byte identical to the old behaviour.
    pub fn fuse_evidence(
        &self,
        verdicts: &[Verdict],
        config: &FusionConfig,
    ) -> Option<FusionResult> {
        if !config.is_enabled() || verdicts.is_empty() {
            return None;
        }

        // Group by axis, preserving a deterministic axis order in the output.
        let mut axis_names: Vec<&'static str> = verdicts.iter().map(|v| v.axis.as_str()).collect();
        axis_names.sort_unstable();
        axis_names.dedup();

        let mut axes: Vec<AxisFusion> = Vec::with_capacity(axis_names.len());
        let mut disagreement_reason: Option<String> = None;

        for axis_name in &axis_names {
            let axis_verdicts: Vec<&Verdict> = verdicts
                .iter()
                .filter(|v| v.axis.as_str() == *axis_name)
                .collect();

            if axis_verdicts.is_empty() {
                continue;
            }

            // Collapse to one contribution per detector, keeping its strongest reading.
            // Without this, two verdicts from the same detector (or an evidence reading
            // and its actionable twin) would double-count the same evidence and inflate
            // the fused probability — the failure mode the plan warns about in §14.
            let mut per_detector: Vec<DetectorContribution> = Vec::new();
            for verdict in &axis_verdicts {
                let base = base_detector(&verdict.check_name);
                let contribution = DetectorContribution {
                    check_name: verdict.check_name.clone(),
                    outcome: verdict.outcome,
                    p: verdict.confidence.clamp(0.0, 1.0),
                    weight: config.fitted_weight(&verdict.check_name).map(|w| w as f32),
                    calibrated: is_judge_detector(&verdict.check_name),
                    verdict_id: verdict.id,
                };

                match per_detector
                    .iter_mut()
                    .find(|existing| base_detector(&existing.check_name) == base)
                {
                    Some(existing) if contribution.p > existing.p => *existing = contribution,
                    Some(_) => {}
                    None => per_detector.push(contribution),
                }
            }

            // Weighted noisy-OR, skipping readings below the evidence floor entirely.
            //
            // A detector with no fitted weight is NOT fused: its verdict keeps exactly the
            // authority it has today (`unfitted_outcome`). Fitting a detector is how the
            // operator says "I know this detector's error profile — apply the calibrated
            // rule to it", so a partial fit can never silently weaken the other checks.
            let mut p_axis = 1.0f64;
            let mut fused: Vec<DetectorContribution> = Vec::new();
            let mut unfitted_outcome = Outcome::Pass;
            let mut detectors: Vec<DetectorContribution> = Vec::new();

            // Thresholds are compared at `f32` precision on purpose: `Verdict.confidence`
            // is an `f32`, and widening 0.45 to `f64` yields 0.449999988..., which would
            // silently drop a reading that is exactly on the boundary.
            let evidence_floor = config.evidence_threshold as f32;

            for contribution in per_detector {
                match contribution.weight {
                    Some(weight) => {
                        if contribution.p < evidence_floor {
                            continue;
                        }
                        p_axis *= 1.0 - (weight as f64) * (contribution.p as f64);
                        detectors.push(contribution.clone());
                        fused.push(contribution);
                    }
                    None => {
                        unfitted_outcome =
                            Outcome::worst(unfitted_outcome, contribution.outcome);
                        detectors.push(contribution);
                    }
                }
            }
            let p_axis = (1.0 - p_axis).clamp(0.0, 1.0);

            let axis_enum = Axis::from_str_loose(axis_name).unwrap_or(Axis::Responsibility);

            let base_outcome = if p_axis >= config.escalate_threshold {
                Outcome::Escalate
            } else if p_axis >= config.edit_threshold {
                Outcome::Edit
            } else {
                Outcome::Pass
            };

            // Corroboration rule: a single heuristic detector may not escalate on its
            // own. Convert the ensemble into a precision improvement rather than an
            // amplifier of one over-eager rule.
            let corroboration_floor = config.corroboration_floor as f32;
            let judge_confidence = config.calibrated_judge_confidence as f32;

            let corroborated = fused
                .iter()
                .filter(|c| c.outcome != Outcome::Pass || c.calibrated)
                .filter(|c| c.p >= corroboration_floor)
                .count()
                >= 2
                || fused
                    .iter()
                    .any(|c| c.calibrated && c.p >= judge_confidence);

            // NB: `Outcome` is ordered Pass < Escalate < Edit < Block, so "worse" is
            // `Outcome::worst`, never `max`, and severity must be matched by name.
            let fused_outcome = if matches!(base_outcome, Outcome::Escalate | Outcome::Block)
                && !corroborated
            {
                Outcome::Edit
            } else {
                base_outcome
            };

            let mut outcome = Outcome::worst(fused_outcome, unfitted_outcome);

            // Disagreement routing (§5.6): when the calibrated judge and the heuristics
            // are materially apart on this axis, the case is one where a human adds the
            // most value — and recording the conflict is what generates the next round of
            // labelled data for the weight fit.
            let heuristic_p = detectors
                .iter()
                .filter(|c| !c.calibrated)
                .map(|c| c.p)
                .fold(0.0f32, f32::max);
            let judge_p = detectors
                .iter()
                .filter(|c| c.calibrated)
                .map(|c| c.p)
                .fold(0.0f32, f32::max);
            let delta = (heuristic_p - judge_p).abs();

            let axis_disagrees =
                heuristic_p > 0.0 && judge_p > 0.0 && delta >= config.disagreement_delta as f32;

            if axis_disagrees {
                outcome = escalate_at_least(outcome);
                let reason = format!(
                    "Judge/heuristic disagreement on the {axis_name} axis: heuristic p={heuristic_p:.2} vs judge p={judge_p:.2} (delta {delta:.2}) — routed to human review."
                );
                if disagreement_reason.is_none() {
                    disagreement_reason = Some(reason.clone());
                }
            }

            axes.push(AxisFusion {
                axis: axis_enum,
                p: p_axis as f32,
                outcome,
                detectors,
            });
        }

        if axes.is_empty() {
            return None;
        }

        let final_outcome = axes
            .iter()
            .map(|a| a.outcome)
            .fold(Outcome::Pass, Outcome::worst);
        let triggered_axes: Vec<String> = axes
            .iter()
            .filter(|a| a.outcome != Outcome::Pass)
            .map(|a| a.axis.as_str().to_string())
            .collect();

        // Preserve the existing engine's compound-risk escalation: two or more axes
        // firing is itself evidence of overlap, even when each axis stayed below its
        // escalation threshold.
        let mut final_outcome = final_outcome;
        if triggered_axes.len() >= 2 && matches!(final_outcome, Outcome::Pass | Outcome::Edit) {
            final_outcome = Outcome::Escalate;
        }

        let primary_axis = axes
            .iter()
            .filter(|a| a.outcome == final_outcome)
            .max_by(|a, b| a.p.partial_cmp(&b.p).unwrap_or(std::cmp::Ordering::Equal))
            .or_else(|| axes.iter().max_by(|a, b| a.p.partial_cmp(&b.p).unwrap_or(std::cmp::Ordering::Equal)))?;

        let contributing_ids: Vec<Uuid> = verdicts
            .iter()
            .filter(|v| v.outcome != Outcome::Pass)
            .map(|v| v.id)
            .collect();

        let primary_reason = match &disagreement_reason {
            Some(reason) => format!(
                "{reason} Fused p_{}={:.2} across {} detector(s).",
                primary_axis.axis.as_str(),
                primary_axis.p,
                primary_axis.detectors.len()
            ),
            None => {
                let loudest = primary_axis
                    .detectors
                    .iter()
                    .max_by(|a, b| a.p.partial_cmp(&b.p).unwrap_or(std::cmp::Ordering::Equal));

                match loudest {
                    Some(detector) => verdicts
                        .iter()
                        .find(|v| v.id == detector.verdict_id)
                        .map(|v| v.reason.clone())
                        .unwrap_or_else(|| {
                            format!(
                                "Fused p_{}={:.2} on the {} axis",
                                primary_axis.axis.as_str(),
                                primary_axis.p,
                                primary_axis.axis.as_str()
                            )
                        }),
                    None => format!(
                        "Fused p_{}={:.2}; no detector cleared the evidence floor",
                        primary_axis.axis.as_str(),
                        primary_axis.p
                    ),
                }
            }
        };

        let primary_reason = match config.calibration_version {
            Some(version) => format!("{primary_reason} [calibrated fusion v{version}]"),
            None => primary_reason,
        };

        let aggregation = AggregationResult {
            final_outcome,
            primary_reason,
            contributing_ids,
            confidence: primary_axis.p,
            compound_risk: triggered_axes.len() >= 2,
            triggered_axes,
        };

        Some(FusionResult {
            aggregation,
            axes,
            disagreement: disagreement_reason.is_some(),
            disagreement_reason,
            calibration_version: config.calibration_version,
        })
    }
}

impl Default for VerdictAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct AggregationResult {
    pub final_outcome: Outcome,
    pub primary_reason: String,
    pub contributing_ids: Vec<Uuid>,
    pub confidence: f32,
    pub compound_risk: bool,
    pub triggered_axes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use controlplane_common::types::{Axis, Path as VerdictPath};

    fn make_verdict(outcome: Outcome, confidence: f32, reason: &str) -> Verdict {
        Verdict::new(
            Uuid::nil(),
            Axis::Responsibility,
            VerdictPath::Fast,
            outcome,
            confidence,
            reason,
            "test_check",
        )
    }

    #[test]
    fn worst_outcome_wins() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Pass, 1.0, "All good"),
            make_verdict(Outcome::Escalate, 0.8, "Bias detected"),
            make_verdict(Outcome::Edit, 0.9, "PII found"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        // Edit > Escalate > Pass
        assert_eq!(result.final_outcome, Outcome::Edit);
    }

    #[test]
    fn block_overrides_everything() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Edit, 0.95, "PII found"),
            make_verdict(Outcome::Block, 0.7, "Unsafe content"),
            make_verdict(Outcome::Escalate, 0.9, "Bias"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        assert_eq!(result.final_outcome, Outcome::Block);
        assert!(result.primary_reason.contains("Unsafe"));
    }

    #[test]
    fn highest_confidence_is_primary() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Block, 0.7, "Low confidence block"),
            make_verdict(Outcome::Block, 0.95, "High confidence block"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        assert_eq!(result.confidence, 0.95);
        assert!(result.primary_reason.contains("High confidence"));
    }

    #[test]
    fn empty_verdicts_pass() {
        let aggregator = VerdictAggregator::new();
        let result = aggregator.aggregate_with_reasoning(&[]);
        assert_eq!(result.final_outcome, Outcome::Pass);
    }

    #[test]
    fn all_pass_gives_pass() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![
            make_verdict(Outcome::Pass, 1.0, "OK"),
            make_verdict(Outcome::Pass, 1.0, "Also OK"),
        ];

        let result = aggregator.aggregate_with_reasoning(&verdicts);
        assert_eq!(result.final_outcome, Outcome::Pass);
        assert!(result.contributing_ids.is_empty());
    }

    #[test]
    fn decision_from_verdicts() {
        let aggregator = VerdictAggregator::new();
        let app_id = Uuid::now_v7();
        let call_id = Uuid::now_v7();
        let verdicts = vec![
            make_verdict(Outcome::Escalate, 0.8, "Issue"),
        ];

        let decision = aggregator.aggregate(call_id, app_id, &verdicts, Some(3));
        assert_eq!(decision.call_id, call_id);
        assert_eq!(decision.app_id, app_id);
        assert_eq!(decision.final_outcome, Outcome::Escalate);
        assert_eq!(decision.applied_policy_version, Some(3));
    }

    // ── Calibrated fusion (plan §5.2 / §5.6) ──────────────────────────────────────

    fn detector(
        check_name: &str,
        axis: Axis,
        outcome: Outcome,
        confidence: f32,
    ) -> Verdict {
        Verdict::new(
            Uuid::nil(),
            axis,
            VerdictPath::Shadow,
            outcome,
            confidence,
            &format!("reason for {check_name}"),
            check_name,
        )
    }

    fn config_with(weights: &[(&str, f64)]) -> FusionConfig {
        let mut config = FusionConfig::default();
        for (detector, weight) in weights {
            config.weights.insert((*detector).to_string(), *weight);
        }
        config
    }

    fn fuse(verdicts: &[Verdict], config: &FusionConfig) -> FusionResult {
        VerdictAggregator::new()
            .fuse_evidence(verdicts, config)
            .expect("fusion must be enabled for this config")
    }

    fn axis_p(result: &FusionResult, axis: Axis) -> f32 {
        result
            .axes
            .iter()
            .find(|a| a.axis == axis)
            .map(|a| a.p)
            .unwrap_or(0.0)
    }

    #[test]
    fn evidence_suffix_matches_the_shadow_path_convention() {
        // Pinned deliberately: the decision crate does not depend on the shadow crate, so
        // this literal is the contract between them.
        assert_eq!(EVIDENCE_SUFFIX, "-evidence");
        assert_eq!(base_detector("laya-bias-evidence"), "laya-bias");
        assert_eq!(base_detector("laya-bias"), "laya-bias");
        assert_eq!(base_detector("bias_classification"), "bias_classification");
        assert!(is_judge_detector("laya-bias-evidence"));
        assert!(!is_judge_detector("bias_classification"));
    }

    #[test]
    fn fusion_is_off_without_fitted_weights() {
        let aggregator = VerdictAggregator::new();
        let verdicts = vec![detector(
            "bias_classification",
            Axis::Responsibility,
            Outcome::Escalate,
            0.9,
        )];

        // No fitted rows -> no calibrated inputs -> the caller must fall back.
        assert!(aggregator
            .fuse_evidence(&verdicts, &FusionConfig::default())
            .is_none());
        assert!(!FusionConfig::default().is_enabled());
    }

    #[test]
    fn fusion_without_verdicts_is_none() {
        let config = config_with(&[("bias_classification", 0.5)]);
        assert!(VerdictAggregator::new()
            .fuse_evidence(&[], &config)
            .is_none());
    }

    #[test]
    fn weighted_noisy_or_combines_independent_detectors() {
        let verdicts = vec![
            detector("prompt_injection", Axis::Responsibility, Outcome::Escalate, 0.9),
            detector("semantic_pii", Axis::Responsibility, Outcome::Escalate, 0.9),
        ];
        let config = config_with(&[("prompt_injection", 0.6), ("semantic_pii", 0.6)]);

        let result = fuse(&verdicts, &config);

        // 1 - (1 - 0.54)^2 = 0.7884
        assert!(
            (axis_p(&result, Axis::Responsibility) - 0.7884).abs() < 0.001,
            "got {}",
            axis_p(&result, Axis::Responsibility)
        );
        assert_eq!(result.aggregation.final_outcome, Outcome::Edit);
        assert!(!result.disagreement);
    }

    #[test]
    fn a_single_heuristic_detector_is_capped_at_edit() {
        // The precision half of the plan: one over-eager rule may not escalate alone.
        let verdicts = vec![detector(
            "bias_classification",
            Axis::Responsibility,
            Outcome::Escalate,
            0.95,
        )];
        let config = config_with(&[("bias_classification", 1.0)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.aggregation.final_outcome, Outcome::Edit);
        assert!(result.disagreement_reason.is_none());
    }

    #[test]
    fn two_moderate_detectors_corroborate_and_may_escalate() {
        let verdicts = vec![
            detector("bias_classification", Axis::Responsibility, Outcome::Escalate, 0.95),
            detector("input-bias", Axis::Responsibility, Outcome::Escalate, 0.6),
        ];
        let config = config_with(&[("bias_classification", 1.0), ("input-bias", 0.6)]);

        let result = fuse(&verdicts, &config);

        // 1 - (1-0.95)(1-0.36) = 0.968 -> Escalate, corroborated by two detectors >= 0.5.
        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
    }

    #[test]
    fn a_confident_calibrated_judge_acts_without_corroboration() {
        let verdicts = vec![detector(
            "laya-bias",
            Axis::Responsibility,
            Outcome::Escalate,
            0.95,
        )];
        let config = config_with(&[("laya-bias", 1.0)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
        assert!(result.axes[0].detectors[0].calibrated);
    }

    #[test]
    fn judge_and_heuristic_disagreement_is_routed_to_a_human() {
        // Heuristic fires hard; the calibrated judge says it is nearly clean. That is
        // exactly the case where a reviewer adds the most value (§5.6).
        let verdicts = vec![
            detector("prompt_injection", Axis::Responsibility, Outcome::Escalate, 0.95),
            detector("laya-prompt-injection-evidence", Axis::Responsibility, Outcome::Pass, 0.45),
        ];
        let config = config_with(&[("prompt_injection", 1.0), ("laya-prompt-injection", 0.5)]);

        let result = fuse(&verdicts, &config);

        assert!(result.disagreement, "{result:?}");
        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
        let reason = result.disagreement_reason.unwrap();
        assert!(reason.contains("disagreement"), "reason: {reason}");
        assert!(reason.contains("responsibility"), "reason: {reason}");
        assert!(result.aggregation.primary_reason.contains("disagreement"));
    }

    #[test]
    fn a_close_agreement_is_not_treated_as_disagreement() {
        let verdicts = vec![
            detector("prompt_injection", Axis::Responsibility, Outcome::Edit, 0.80),
            detector("laya-prompt-injection-evidence", Axis::Responsibility, Outcome::Pass, 0.70),
        ];
        let config = config_with(&[("prompt_injection", 1.0), ("laya-prompt-injection", 0.5)]);

        let result = fuse(&verdicts, &config);
        assert!(!result.disagreement, "delta 0.10 must not escalate: {result:?}");
    }

    #[test]
    fn readings_below_the_evidence_floor_are_discarded() {
        let verdicts = vec![detector(
            "bias_classification",
            Axis::Responsibility,
            Outcome::Pass,
            0.30,
        )];
        let config = config_with(&[("bias_classification", 1.0)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(axis_p(&result, Axis::Responsibility), 0.0);
        assert!(result.axes[0].detectors.is_empty());
        assert_eq!(result.aggregation.final_outcome, Outcome::Pass);
    }

    #[test]
    fn one_detector_never_contributes_twice() {
        // Double-counting the same evidence is the classic way an ensemble inflates
        // confidence. Only the strongest reading per detector is fused.
        let verdicts = vec![
            detector("bias_classification", Axis::Responsibility, Outcome::Escalate, 0.9),
            detector("bias_classification", Axis::Responsibility, Outcome::Edit, 0.6),
        ];
        let config = config_with(&[("bias_classification", 0.5)]);

        let result = fuse(&verdicts, &config);

        assert!(
            (axis_p(&result, Axis::Responsibility) - 0.45).abs() < 0.001,
            "got {}",
            axis_p(&result, Axis::Responsibility)
        );
        assert_eq!(result.axes[0].detectors.len(), 1);
    }

    #[test]
    fn an_evidence_reading_uses_its_detectors_weight() {
        let verdicts = vec![detector(
            "laya-bias-evidence",
            Axis::Responsibility,
            Outcome::Pass,
            0.90,
        )];
        let config = config_with(&[("laya-bias", 0.8)]);

        let result = fuse(&verdicts, &config);

        // With the suffix stripped, the fitted 0.8 weight applies: 1 - (1 - 0.72) = 0.72.
        // Without stripping it would fall back to the 0.3 default and give 0.27.
        assert_eq!(result.axes[0].detectors[0].weight, Some(0.8));
        assert!(
            (axis_p(&result, Axis::Responsibility) - 0.72).abs() < 0.001,
            "got {}",
            axis_p(&result, Axis::Responsibility)
        );
    }

    #[test]
    fn a_partial_fit_never_weakens_the_detectors_it_does_not_cover() {
        // The safety property that matters most: fitting only the judge must leave every
        // existing heuristic with exactly the authority it has today.
        let verdicts = vec![detector(
            "bias_classification",
            Axis::Responsibility,
            Outcome::Escalate,
            0.85,
        )];
        let config = config_with(&[("laya-bias", 0.8)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.axes[0].detectors[0].weight, None, "outside the fit");
        assert_eq!(
            result.aggregation.final_outcome,
            Outcome::Escalate,
            "an unfitted detector must keep its existing authority"
        );
    }

    #[test]
    fn an_unfitted_detector_still_participates_in_disagreement() {
        let verdicts = vec![
            detector("bias_classification", Axis::Responsibility, Outcome::Escalate, 0.95),
            detector("laya-bias-evidence", Axis::Responsibility, Outcome::Pass, 0.45),
        ];
        let config = config_with(&[("laya-bias", 0.6)]);

        let result = fuse(&verdicts, &config);

        assert!(result.disagreement, "{result:?}");
        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
    }

    #[test]
    fn unfitted_verdicts_do_not_double_count_with_fitted_ones() {
        // Both readings come from the same detector, so only one is considered — and the
        // surviving reading is outside the fit, so it passes through unfused.
        let verdicts = vec![
            detector("bias_classification", Axis::Responsibility, Outcome::Escalate, 0.9),
            detector("bias_classification", Axis::Responsibility, Outcome::Edit, 0.5),
        ];
        let config = config_with(&[("laya-bias", 0.5)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.axes[0].detectors.len(), 1);
        assert_eq!(axis_p(&result, Axis::Responsibility), 0.0);
        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
    }

    #[test]
    fn compound_risk_across_axes_still_escalates() {
        let verdicts = vec![
            detector("laya-bias", Axis::Responsibility, Outcome::Edit, 0.75),
            detector("laya-groundedness", Axis::Performance, Outcome::Edit, 0.75),
        ];
        let config = config_with(&[("laya-bias", 1.0), ("laya-groundedness", 1.0)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
        assert!(result.aggregation.compound_risk);
        assert_eq!(result.aggregation.triggered_axes.len(), 2);
    }

    #[test]
    fn fusion_records_the_calibration_version() {
        let verdicts = vec![detector("laya-bias", Axis::Responsibility, Outcome::Escalate, 0.95)];
        let mut config = config_with(&[("laya-bias", 1.0)]);
        config.calibration_version = Some(4);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.calibration_version, Some(4));
        assert!(
            result.aggregation.primary_reason.contains("fusion v4"),
            "reason: {}",
            result.aggregation.primary_reason
        );
    }

    #[test]
    fn contributing_ids_exclude_evidence_only_readings() {
        let verdicts = vec![
            detector("laya-bias", Axis::Responsibility, Outcome::Edit, 0.8),
            detector("laya-toxicity-evidence", Axis::Responsibility, Outcome::Pass, 0.5),
        ];
        let config = config_with(&[("laya-bias", 1.0), ("laya-toxicity", 0.5)]);

        let result = fuse(&verdicts, &config);

        assert_eq!(result.aggregation.contributing_ids.len(), 1);
    }

    #[test]
    fn fusion_is_deterministic() {
        // AGENTS.md rule 4: the final verdict must be a pure function of the scores and
        // the stored thresholds.
        let verdicts = vec![
            detector("prompt_injection", Axis::Responsibility, Outcome::Escalate, 0.86),
            detector("laya-prompt-injection", Axis::Responsibility, Outcome::Escalate, 0.94),
            detector("groundedness", Axis::Performance, Outcome::Edit, 0.71),
        ];
        let config = config_with(&[
            ("prompt_injection", 0.4),
            ("laya-prompt-injection", 0.6),
            ("groundedness", 0.2),
        ]);

        let first = fuse(&verdicts, &config);
        for _ in 0..5 {
            let again = fuse(&verdicts, &config);
            assert_eq!(again.aggregation.final_outcome, first.aggregation.final_outcome);
            assert_eq!(again.axes.len(), first.axes.len());
            for (a, b) in again.axes.iter().zip(first.axes.iter()) {
                assert_eq!(a.axis, b.axis);
                assert!((a.p - b.p).abs() < 1e-9);
                assert_eq!(a.outcome, b.outcome);
            }
        }
    }

    #[test]
    fn disagreement_never_weakens_an_existing_block() {
        let verdicts = vec![
            detector("unsafe_content", Axis::Responsibility, Outcome::Block, 0.95),
            detector("laya-prompt-injection-evidence", Axis::Responsibility, Outcome::Pass, 0.45),
        ];
        let config = config_with(&[("laya-prompt-injection", 0.6)]);

        let result = fuse(&verdicts, &config);

        assert!(result.disagreement);
        assert_eq!(result.aggregation.final_outcome, Outcome::Block);
    }

    #[test]
    fn thresholds_are_configurable_from_the_policy_store() {
        let verdicts = vec![
            detector("bias_classification", Axis::Responsibility, Outcome::Escalate, 0.9),
            detector("input-bias", Axis::Responsibility, Outcome::Escalate, 0.9),
        ];
        let mut config = config_with(&[("bias_classification", 0.6), ("input-bias", 0.6)]);
        // 0.7884 lands in the edit band by default; a stricter policy escalates it.
        config.escalate_threshold = 0.75;

        let result = fuse(&verdicts, &config);
        assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
    }
}
