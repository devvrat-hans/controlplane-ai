//! Laya / Jev decision-model judge client.
//!
//! Talks to `laya-serve` (or the Jev API) over `POST /v1/systemone` — the Jev-compatible
//! shape — and maps the returned calibrated probabilities into `ShadowVerdict`s.
//!
//! See `docs/analysis/laya-integration-plan.md` (§8 schema, §9 mapping, §5.4 calibration).
//!
//! ## Design constraints
//!
//! - **Shadow path only.** This module is never reachable from `fast-path`.
//! - **The judge scores; it does not decide.** Thresholds here produce per-check verdicts;
//!   the decision engine still aggregates and applies policy.
//! - **Fail open.** Every error path returns an empty verdict list. A judge that is down
//!   must never change the outcome of a call — absence of a shadow verdict means pass.
//! - **Precision over recall in the action mapping.** Between the evidence floor and the
//!   edit threshold the judge emits *nothing actionable*: the reading is published as a
//!   Pass verdict marked `-evidence` so the decision engine can fuse and compare it,
//!   but it cannot on its own edit, escalate or block anything.
//!
//! ## Long responses
//!
//! A response longer than the model window is cut into overlapping windows
//! ([`GovernanceState::windows`]); each window is asked separately and the per-window
//! probabilities are **max-pooled** ([`merge_answers`]) so a finding anywhere in the
//! response surfaces rather than an arbitrary verdict from a silent truncation.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tracing::{debug, warn};

use controlplane_common::types::{Axis, Outcome};

use crate::calibration::{Calibration, Primitive};
use crate::governance_questions::{
    build_questions, GovernanceState, POSITIVE_OPTION, Q_BIAS_CATEGORY, Q_BIAS_PRESENT,
    Q_FILLER_RATIO, Q_GROUNDEDNESS, Q_HALLUCINATION, Q_HALLUCINATION_SEVERITY, Q_INJECTION_ATTEMPT,
    Q_INJECTION_FAMILY, Q_IS_REIDENTIFIABLE, Q_REID_TYPE, Q_TOOL_CALL_RISK, Q_TOXICITY_SEVERITY,
};
use crate::types::ShadowVerdict;

/// Calibrated risk at or above this escalates to a human.
pub const ESCALATE_THRESHOLD: f64 = 0.90;
/// Calibrated risk at or above this edits / annotates the response.
pub const EDIT_THRESHOLD: f64 = 0.70;
/// At or above this the reading is worth keeping as *evidence* for the decision engine's
/// fusion (plan §9: "0.45 – 0.70 -> no verdict, but attach as evidence"). Below it the
/// reading is noise and is discarded.
pub const EVIDENCE_THRESHOLD: f64 = 0.45;

/// Suffix marking a Pass verdict as a sub-threshold evidence reading.
pub const EVIDENCE_SUFFIX: &str = "-evidence";

// ─── Question scales ─────────────────────────────────────────────────────────────

/// How a raw judge answer becomes a calibrated risk, and under which detector name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskKind {
    /// Two-option `choice`: risk is the probability of the unsafe option.
    YesNo,
    /// `score` rubric: risk is the selected level, normalized and polarity-corrected.
    Ordinal,
    /// Multi-option `choice`: risk is the severity rank of the selected category.
    Category,
}

/// One question's mapping + calibration identity.
///
/// Keeping this in a table means the schema and the mapping cannot drift apart, and the
/// offline fit in `scripts/eval_accuracy.sh` can mention exactly the detectors the judge
/// actually emits.
#[derive(Debug, Clone, Copy)]
pub struct QuestionScale {
    /// Question key in the schema (`build_questions`).
    pub key: &'static str,
    /// `check_name` of the verdict this question produces.
    pub detector: &'static str,
    /// Axis the verdict belongs to.
    pub axis: Axis,
    pub kind: RiskKind,
    /// Calibration key components (plan §5.4: fitted per primitive + option bucket).
    pub primitive: Primitive,
    pub option_count: usize,
    /// Rubric levels for `Ordinal`; ignored otherwise.
    pub levels: usize,
    /// `Ordinal` polarity: true for "not toxic -> severe", false for "unsupported -> supported".
    pub higher_is_worse: bool,
    /// Only ask this question when a retrieval context exists.
    pub context_required: bool,
    /// Whether a sub-threshold reading is worth publishing as evidence.
    pub emits_evidence: bool,
}

/// Every question that can drive a decision, with its calibration identity.
pub const QUESTION_SCALES: &[QuestionScale] = &[
    QuestionScale {
        key: Q_HALLUCINATION,
        detector: "laya-hallucination",
        axis: Axis::Performance,
        kind: RiskKind::YesNo,
        primitive: Primitive::Choice,
        option_count: 2,
        levels: 2,
        higher_is_worse: true,
        context_required: true,
        emits_evidence: true,
    },
    QuestionScale {
        key: Q_GROUNDEDNESS,
        detector: "laya-groundedness",
        axis: Axis::Performance,
        kind: RiskKind::Ordinal,
        primitive: Primitive::Score,
        option_count: 3,
        levels: 3,
        // Strong support = low risk, so the polarity is inverted.
        higher_is_worse: false,
        context_required: true,
        emits_evidence: true,
    },
    QuestionScale {
        key: Q_INJECTION_ATTEMPT,
        detector: "laya-prompt-injection",
        axis: Axis::Responsibility,
        kind: RiskKind::YesNo,
        primitive: Primitive::Choice,
        option_count: 2,
        levels: 2,
        higher_is_worse: true,
        context_required: false,
        emits_evidence: true,
    },
    QuestionScale {
        key: Q_TOOL_CALL_RISK,
        detector: "laya-tool-use",
        axis: Axis::Performance,
        kind: RiskKind::Category,
        primitive: Primitive::Choice,
        option_count: 5,
        levels: 5,
        higher_is_worse: true,
        context_required: false,
        // The fast-path presence check is the detector of record here; the judge only
        // grades an action that already exists, so it never contributes raw evidence.
        emits_evidence: false,
    },
    QuestionScale {
        key: Q_BIAS_PRESENT,
        detector: "laya-bias",
        axis: Axis::Responsibility,
        kind: RiskKind::YesNo,
        primitive: Primitive::Choice,
        option_count: 2,
        levels: 2,
        higher_is_worse: true,
        context_required: false,
        emits_evidence: true,
    },
    QuestionScale {
        key: Q_TOXICITY_SEVERITY,
        detector: "laya-toxicity",
        axis: Axis::Responsibility,
        kind: RiskKind::Ordinal,
        primitive: Primitive::Score,
        option_count: 3,
        levels: 3,
        higher_is_worse: true,
        context_required: false,
        emits_evidence: true,
    },
    QuestionScale {
        key: Q_IS_REIDENTIFIABLE,
        detector: "laya-semantic-pii",
        axis: Axis::Responsibility,
        kind: RiskKind::YesNo,
        primitive: Primitive::Choice,
        option_count: 2,
        levels: 2,
        higher_is_worse: true,
        context_required: false,
        emits_evidence: true,
    },
    QuestionScale {
        key: Q_FILLER_RATIO,
        detector: "laya-verbosity",
        axis: Axis::Cost,
        kind: RiskKind::Ordinal,
        primitive: Primitive::Score,
        option_count: 3,
        levels: 3,
        higher_is_worse: true,
        context_required: false,
        // Low-stakes: padding is a cost signal and never escalates, so a sub-threshold
        // reading has nothing to contribute. Plan §4.8 keeps this the lightest hybrid.
        emits_evidence: false,
    },
];

/// Look up a scale by question key. Panics only if the table above is edited
/// inconsistently, which `question_scales_cover_every_emitted_check` guards against.
fn scale_for(key: &str) -> &'static QuestionScale {
    QUESTION_SCALES
        .iter()
        .find(|scale| scale.key == key)
        .expect("every mapped question must have a QuestionScale entry")
}

/// The detail question that qualifies a driving question (e.g. `bias_category` for
/// `bias_present`). Used so max-pooling keeps the detail from the same window that
/// produced the risk, rather than mixing windows.
fn detail_key_for(key: &str) -> Option<&'static str> {
    match key {
        Q_INJECTION_ATTEMPT => Some(Q_INJECTION_FAMILY),
        Q_BIAS_PRESENT => Some(Q_BIAS_CATEGORY),
        Q_IS_REIDENTIFIABLE => Some(Q_REID_TYPE),
        Q_HALLUCINATION => Some(Q_HALLUCINATION_SEVERITY),
        _ => None,
    }
}

/// Severity rank of a tool-action category, normalized to `[0, 1]`.
fn category_rank(label: &str) -> f64 {
    match label {
        "low" => 0.25,
        "medium" => 0.5,
        "high" => 0.75,
        "destructive" => 1.0,
        _ => 0.0,
    }
}

// ─── Transport types ─────────────────────────────────────────────────────────────

/// One typed answer from the judge. Every field is optional because the exact payload
/// varies by primitive (choice / score / noul) and by backend (Laya vs Jev).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LayaAnswer {
    #[serde(rename = "type", default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub choice: Option<String>,
    #[serde(default)]
    pub probabilities: Option<serde_json::Value>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub noul: Option<f64>,
    #[serde(default)]
    pub confidence: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct LayaResponse {
    #[serde(default)]
    answers: HashMap<String, LayaAnswer>,
    #[serde(default)]
    model: Option<String>,
}

/// Client for the decision-model judge.
pub struct LayaClient {
    base_url: String,
    model: Option<String>,
    api_key: Option<String>,
    /// Wall-clock budget for the whole `evaluate` call, across all response windows.
    timeout_ms: u64,
    /// Fitted temperature scales. Inert unless a fit has been written to the DB.
    calibration: Calibration,
    http: reqwest::Client,
}

impl LayaClient {
    pub fn new(
        base_url: &str,
        timeout_ms: u64,
        model: Option<String>,
        api_key: Option<String>,
    ) -> Self {
        let http = reqwest::Client::builder()
            // Hard cap: the shadow path must never hang on the judge.
            .timeout(Duration::from_millis(timeout_ms.max(1)))
            .build()
            .unwrap_or_default();

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            api_key,
            timeout_ms: timeout_ms.max(1),
            calibration: Calibration::inert(),
            http,
        }
    }

    /// Attach fitted calibration parameters. Without this the client operates with the
    /// identity transform, i.e. exactly the pre-calibration behaviour.
    pub fn with_calibration(mut self, calibration: Calibration) -> Self {
        self.calibration = calibration;
        self
    }

    /// Ask the judge every governance question for this call.
    ///
    /// Laya answers all questions for one `state` in a single forward pass, so a response
    /// that fits the window costs exactly **one** HTTP call. A longer response costs one
    /// call per window (bounded by `MAX_RESPONSE_WINDOWS`), each within the same overall
    /// time budget.
    ///
    /// Returns an empty list on any failure — fail-open by construction.
    pub async fn evaluate(&self, state: &GovernanceState) -> Vec<ShadowVerdict> {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(self.timeout_ms);
        let windows = state.windows();

        let mut answered: Vec<HashMap<String, LayaAnswer>> = Vec::new();
        let mut model: Option<String> = None;

        for (index, window) in windows.iter().enumerate() {
            // Stop issuing windows once the shared budget is spent rather than letting a
            // pathological response multiply the shadow path's latency.
            if index > 0 && Instant::now() >= deadline {
                warn!(
                    windows = windows.len(),
                    answered = answered.len(),
                    "Laya judge window budget exhausted — analysing the windows already answered"
                );
                break;
            }

            if let Some((answers, window_model)) = self.evaluate_window(window).await {
                model = window_model.or(model);
                answered.push(answers);
            }
        }

        if answered.is_empty() {
            return Vec::new();
        }

        let merged = if answered.len() == 1 {
            answered.pop().unwrap_or_default()
        } else {
            merge_answers(&answered, &self.calibration)
        };

        let duration_ms = started.elapsed().as_millis() as u32;
        debug!(
            answers = merged.len(),
            model = ?model,
            windows = windows.len(),
            windows_answered = answered.len(),
            duration_ms,
            "Laya judge complete"
        );

        map_answers_calibrated(
            &merged,
            state.has_context(),
            state.is_truncated(),
            duration_ms,
            &self.calibration,
            windows.len(),
        )
    }

    /// One HTTP round-trip for one window. `None` on any failure, after logging.
    async fn evaluate_window(
        &self,
        state: &GovernanceState,
    ) -> Option<(HashMap<String, LayaAnswer>, Option<String>)> {
        let url = format!("{}/v1/systemone", self.base_url);

        let mut body = serde_json::json!({
            "state": state.to_json(),
            "questions": build_questions(state.has_context()),
        });
        if let Some(model) = &self.model {
            body["model"] = serde_json::json!(model);
        }

        let mut request = self.http.post(&url).json(&body);
        if let Some(api_key) = &self.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = match request.send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                warn!(status = %response.status(), "Laya judge returned an error status — FAIL OPEN for this window");
                return None;
            }
            Err(e) => {
                warn!(error = %e, "Laya judge unreachable — FAIL OPEN for this window");
                return None;
            }
        };

        match response.json::<LayaResponse>().await {
            Ok(parsed) => Some((parsed.answers, parsed.model)),
            Err(e) => {
                warn!(error = %e, "Failed to parse Laya judge response — FAIL OPEN for this window");
                None
            }
        }
    }
}

// ─── Pure mapping (no I/O — unit-testable in isolation) ──────────────────────────

/// Probability the judge assigned to a specific option label.
///
/// Prefers the full distribution when present. Falls back to `confidence` for the chosen
/// option, and to `1 - confidence` for the other side of a two-option question (an
/// approximation that is only used when the distribution is missing).
fn probability_of(answer: &LayaAnswer, label: &str) -> Option<f64> {
    if let Some(probabilities) = &answer.probabilities {
        if let Some(p) = probabilities.get(label).and_then(|v| v.as_f64()) {
            return Some(p.clamp(0.0, 1.0));
        }
    }

    match (answer.choice.as_deref(), answer.confidence) {
        (Some(chosen), Some(confidence)) if chosen == label => Some(confidence.clamp(0.0, 1.0)),
        (Some(_), Some(confidence)) => Some((1.0 - confidence).clamp(0.0, 1.0)),
        _ => None,
    }
}

/// Probability of whichever option the judge actually chose.
fn selected_probability(answer: &LayaAnswer) -> Option<f64> {
    let chosen = answer.choice.as_deref()?;
    probability_of(answer, chosen)
}

/// Risk from an ordinal rubric, normalized to `[0, 1]`.
///
/// `levels` is the number of rubric criteria. `higher_is_worse` selects the polarity:
/// true for "not toxic -> severe", false for "not supported -> well supported".
fn ordinal_risk(answer: &LayaAnswer, levels: usize, higher_is_worse: bool) -> Option<(f64, f64)> {
    let score = answer.score?;
    let denominator = levels.saturating_sub(1) as f64;
    if denominator <= 0.0 {
        return None;
    }

    let normalized = (score / denominator).clamp(0.0, 1.0);
    let risk = if higher_is_worse {
        normalized
    } else {
        1.0 - normalized
    };
    let confidence = answer.confidence.unwrap_or(normalized).clamp(0.0, 1.0);

    Some((risk, confidence))
}

/// The raw, uncalibrated risk for a scale — the input to temperature scaling.
fn raw_risk(scale: &QuestionScale, answer: &LayaAnswer) -> Option<f64> {
    match scale.kind {
        RiskKind::YesNo => probability_of(answer, POSITIVE_OPTION),
        RiskKind::Ordinal => ordinal_risk(answer, scale.levels, scale.higher_is_worse).map(|(r, _)| r),
        RiskKind::Category => {
            let label = answer.choice.as_deref().unwrap_or_default();
            Some(category_rank(label))
        }
    }
}

/// The calibrated risk for a scale — what the verdict's `confidence` stores
/// (plan §9) and what the decision engine's fusion consumes.
fn calibrated_risk(
    scale: &QuestionScale,
    answer: &LayaAnswer,
    calibration: &Calibration,
) -> Option<f64> {
    let raw = raw_risk(scale, answer)?;
    Some(calibration.apply(scale.detector, scale.primitive, scale.option_count, raw))
}

/// Map a calibrated risk to an outcome, or `None` when the judge is not confident enough
/// to act. Emitting nothing actionable below the edit threshold is deliberate — it is
/// what protects precision.
fn classify(risk: f64) -> Option<Outcome> {
    if risk >= ESCALATE_THRESHOLD {
        Some(Outcome::Escalate)
    } else if risk >= EDIT_THRESHOLD {
        Some(Outcome::Edit)
    } else {
        None
    }
}

/// The category label chosen by a category question, ignoring the `other` escape hatch.
fn category_label(answer: Option<&LayaAnswer>) -> Option<String> {
    answer?.choice.clone().filter(|choice| choice != "other")
}

/// Max-pool the per-window answers.
///
/// For every driving question, keep the window whose reading is **riskiest** ("a finding
/// anywhere in the response must surface"), and take that question's detail answer from
/// the same window so the explanation matches the reading.
///
/// Pure function — no I/O — so the pooling behaviour is directly testable.
pub fn merge_answers(
    windows: &[HashMap<String, LayaAnswer>],
    calibration: &Calibration,
) -> HashMap<String, LayaAnswer> {
    let mut merged: HashMap<String, LayaAnswer> = HashMap::new();

    for scale in QUESTION_SCALES {
        let mut best: Option<(usize, f64)> = None;

        for (index, answers) in windows.iter().enumerate() {
            let Some(answer) = answers.get(scale.key) else {
                continue;
            };
            let Some(risk) = calibrated_risk(scale, answer, calibration) else {
                continue;
            };

            if best.is_none_or(|(_, best_risk)| risk > best_risk) {
                best = Some((index, risk));
            }
        }

        let Some((index, _)) = best else { continue };

        if let Some(answer) = windows[index].get(scale.key) {
            merged.insert(scale.key.to_string(), answer.clone());
        }
        if let Some(detail_key) = detail_key_for(scale.key) {
            if let Some(detail) = windows[index].get(detail_key) {
                merged.insert(detail_key.to_string(), detail.clone());
            }
        }
    }

    merged
}

/// Publish a sub-threshold reading as evidence-only.
///
/// Plan §9: the 0.45 – 0.70 band produces "no verdict, but attach as evidence for
/// fusion". The verdict is `Outcome::Pass`, so it can never on its own edit, escalate or
/// block anything — it exists so the decision engine can fuse it and so a reviewer can
/// see where the judge and the heuristics disagree.
fn push_evidence(
    verdicts: &mut Vec<ShadowVerdict>,
    scale: &QuestionScale,
    risk: f64,
    reason: &str,
    suffix: &str,
    duration_ms: u32,
) {
    if !scale.emits_evidence || risk < EVIDENCE_THRESHOLD || risk >= EDIT_THRESHOLD {
        return;
    }

    verdicts.push(ShadowVerdict {
        axis: scale.axis,
        check_name: format!("{}{}", scale.detector, EVIDENCE_SUFFIX),
        outcome: Outcome::Pass,
        confidence: risk as f32,
        reason: format!("{reason} Below the edit threshold, so no action was taken{suffix}"),
        duration_ms,
    });
}

/// Convert raw judge answers into shadow verdicts.
///
/// Pure function — no network, no clock — so every threshold band can be tested directly.
pub fn map_answers(
    answers: &HashMap<String, LayaAnswer>,
    has_context: bool,
    truncated: bool,
    duration_ms: u32,
) -> Vec<ShadowVerdict> {
    map_answers_calibrated(
        answers,
        has_context,
        truncated,
        duration_ms,
        &Calibration::inert(),
        1,
    )
}

/// Convert raw judge answers into shadow verdicts, applying fitted calibration and
/// recording how many response windows were analysed.
///
/// Pure function — no network, no clock.
pub fn map_answers_calibrated(
    answers: &HashMap<String, LayaAnswer>,
    has_context: bool,
    truncated: bool,
    duration_ms: u32,
    calibration: &Calibration,
    window_count: usize,
) -> Vec<ShadowVerdict> {
    let mut verdicts: Vec<ShadowVerdict> = Vec::new();
    let mut suffix = String::new();
    if truncated {
        suffix.push_str(" [input truncated to model window]");
    }
    if window_count > 1 {
        suffix.push_str(&format!(" [{window_count} windows max-pooled]"));
    }

    // ── Performance: hallucination (requires context to compare against) ──────────
    if has_context {
        let scale = scale_for(Q_HALLUCINATION);
        if let Some(answer) = answers.get(Q_HALLUCINATION) {
            if let Some(risk) = calibrated_risk(scale, answer, calibration) {
                let severity = answers
                    .get(Q_HALLUCINATION_SEVERITY)
                    .and_then(|a| ordinal_risk(a, 3, true))
                    .map(|(risk, _)| risk);

                // A material fabrication escalates regardless of the binary answer's margin.
                let outcome = match severity {
                    Some(severity) if severity >= 0.66 => Some(Outcome::Escalate),
                    _ => classify(risk),
                };

                if let Some(outcome) = outcome {
                    verdicts.push(ShadowVerdict {
                        axis: scale.axis,
                        check_name: scale.detector.to_string(),
                        outcome,
                        confidence: risk as f32,
                        reason: format!(
                            "Laya judge: {:.0}% probability the response states facts unsupported by the context{}",
                            risk * 100.0,
                            severity
                                .map(|s| format!(" (severity {s:.2})"))
                                .unwrap_or_default(),
                        ) + &suffix,
                        duration_ms,
                    });
                } else {
                    push_evidence(
                        &mut verdicts,
                        scale,
                        risk,
                        &format!(
                            "Laya judge: {:.0}% probability the response states facts unsupported by the context.",
                            risk * 100.0
                        ),
                        &suffix,
                        duration_ms,
                    );
                }
            }
        }

        // ── Performance: groundedness (inverted polarity — strong support = low risk) ──
        let scale = scale_for(Q_GROUNDEDNESS);
        if let Some(answer) = answers.get(Q_GROUNDEDNESS) {
            if let Some(risk) = calibrated_risk(scale, answer, calibration) {
                match classify(risk) {
                    Some(outcome) => verdicts.push(ShadowVerdict {
                        axis: scale.axis,
                        check_name: scale.detector.to_string(),
                        outcome,
                        confidence: risk as f32,
                        reason: format!(
                            "Laya judge: response is poorly grounded in the provided context (risk {:.0}%){}",
                            risk * 100.0, suffix
                        ),
                        duration_ms,
                    }),
                    None => push_evidence(
                        &mut verdicts,
                        scale,
                        risk,
                        &format!(
                            "Laya judge: response is only partly grounded in the provided context (risk {:.0}%).",
                            risk * 100.0
                        ),
                        &suffix,
                        duration_ms,
                    ),
                }
            }
        }
    }

    // ── Responsibility: prompt injection ─────────────────────────────────────────
    let scale = scale_for(Q_INJECTION_ATTEMPT);
    if let Some(answer) = answers.get(Q_INJECTION_ATTEMPT) {
        if let Some(risk) = calibrated_risk(scale, answer, calibration) {
            let family = category_label(answers.get(Q_INJECTION_FAMILY));
            let reason = format!(
                "Laya judge: {:.0}% probability the prompt attempts to override or extract instructions{}",
                risk * 100.0,
                family
                    .map(|f| format!(" (family: {f})"))
                    .unwrap_or_default(),
            );

            match classify(risk) {
                Some(outcome) => verdicts.push(ShadowVerdict {
                    axis: scale.axis,
                    check_name: scale.detector.to_string(),
                    outcome,
                    confidence: risk as f32,
                    reason: reason + &suffix,
                    duration_ms,
                }),
                None => push_evidence(&mut verdicts, scale, risk, &format!("{reason}."), &suffix, duration_ms),
            }
        }
    }

    // ── Performance: tool / agent action risk ────────────────────────────────────
    let scale = scale_for(Q_TOOL_CALL_RISK);
    if let Some(answer) = answers.get(Q_TOOL_CALL_RISK) {
        let label = answer.choice.as_deref().unwrap_or_default();
        let outcome = match label {
            "destructive" | "high" => Some(Outcome::Escalate),
            "medium" => Some(Outcome::Edit),
            _ => None,
        };

        if let Some(outcome) = outcome {
            let confidence = selected_probability(answer)
                .map(|p| calibration.apply(scale.detector, scale.primitive, scale.option_count, p))
                .unwrap_or(0.5);
            verdicts.push(ShadowVerdict {
                axis: scale.axis,
                check_name: scale.detector.to_string(),
                outcome,
                confidence: confidence as f32,
                reason: format!(
                    "Laya judge: requested tool action rated '{label}' ({:.0}% confident){}",
                    confidence * 100.0,
                    suffix
                ),
                duration_ms,
            });
        }
    }

    // ── Responsibility: bias ─────────────────────────────────────────────────────
    let scale = scale_for(Q_BIAS_PRESENT);
    if let Some(answer) = answers.get(Q_BIAS_PRESENT) {
        if let Some(risk) = calibrated_risk(scale, answer, calibration) {
            let category = category_label(answers.get(Q_BIAS_CATEGORY));
            let reason = format!(
                "Laya judge: {:.0}% probability the text treats a group unfairly{}",
                risk * 100.0,
                category
                    .map(|c| format!(" (category: {c})"))
                    .unwrap_or_default(),
            );

            match classify(risk) {
                Some(outcome) => verdicts.push(ShadowVerdict {
                    axis: scale.axis,
                    check_name: scale.detector.to_string(),
                    outcome,
                    confidence: risk as f32,
                    reason: reason + &suffix,
                    duration_ms,
                }),
                None => push_evidence(&mut verdicts, scale, risk, &format!("{reason}."), &suffix, duration_ms),
            }
        }
    }

    // ── Responsibility: toxicity severity ────────────────────────────────────────
    // Only a severe rating acts. The specialist toxic-roberta model owns the fine-grained
    // middle of this scale, so the judge contributes the top tier only.
    let scale = scale_for(Q_TOXICITY_SEVERITY);
    if let Some(answer) = answers.get(Q_TOXICITY_SEVERITY) {
        if let Some(risk) = calibrated_risk(scale, answer, calibration) {
            match classify(risk) {
                Some(outcome) => verdicts.push(ShadowVerdict {
                    axis: scale.axis,
                    check_name: scale.detector.to_string(),
                    outcome,
                    confidence: risk as f32,
                    reason: format!(
                        "Laya judge: severe toxicity detected (risk {:.0}%){}",
                        risk * 100.0,
                        suffix
                    ),
                    duration_ms,
                }),
                None => push_evidence(
                    &mut verdicts,
                    scale,
                    risk,
                    &format!(
                        "Laya judge: mild toxicity reading (risk {:.0}%).",
                        risk * 100.0
                    ),
                    &suffix,
                    duration_ms,
                ),
            }
        }
    }

    // ── Responsibility: contextual re-identification ─────────────────────────────
    let scale = scale_for(Q_IS_REIDENTIFIABLE);
    if let Some(answer) = answers.get(Q_IS_REIDENTIFIABLE) {
        if let Some(risk) = calibrated_risk(scale, answer, calibration) {
            let kind = category_label(answers.get(Q_REID_TYPE));
            let reason = format!(
                "Laya judge: {:.0}% probability the text could identify a specific person{}",
                risk * 100.0,
                kind.map(|k| format!(" (type: {k})")).unwrap_or_default(),
            );

            match classify(risk) {
                Some(outcome) => verdicts.push(ShadowVerdict {
                    axis: scale.axis,
                    check_name: scale.detector.to_string(),
                    outcome,
                    confidence: risk as f32,
                    reason: reason + &suffix,
                    duration_ms,
                }),
                None => push_evidence(&mut verdicts, scale, risk, &format!("{reason}."), &suffix, duration_ms),
            }
        }
    }

    // ── Cost: verbosity (capped at Edit — padding is a cost signal, not a safety one) ──
    let scale = scale_for(Q_FILLER_RATIO);
    if let Some(answer) = answers.get(Q_FILLER_RATIO) {
        if let Some(risk) = calibrated_risk(scale, answer, calibration) {
            if risk >= EDIT_THRESHOLD {
                verdicts.push(ShadowVerdict {
                    axis: scale.axis,
                    check_name: scale.detector.to_string(),
                    outcome: Outcome::Edit,
                    confidence: risk as f32,
                    reason: format!(
                        "Laya judge: response is padded with filler (risk {:.0}%){}",
                        risk * 100.0,
                        suffix
                    ),
                    duration_ms,
                });
            }
        }
    }

    verdicts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(pairs: &[(&str, LayaAnswer)]) -> HashMap<String, LayaAnswer> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    /// A two-option A/B answer with a full distribution.
    fn ab(probability_positive: f64) -> LayaAnswer {
        LayaAnswer {
            kind: Some("choice".to_string()),
            choice: Some(
                if probability_positive >= 0.5 {
                    "B"
                } else {
                    "A"
                }
                .to_string(),
            ),
            probabilities: Some(serde_json::json!({
                "A": 1.0 - probability_positive,
                "B": probability_positive,
            })),
            confidence: Some(probability_positive.max(1.0 - probability_positive)),
            ..Default::default()
        }
    }

    fn rubric(score: f64, confidence: f64) -> LayaAnswer {
        LayaAnswer {
            kind: Some("score".to_string()),
            score: Some(score),
            confidence: Some(confidence),
            ..Default::default()
        }
    }

    fn category(label: &str, confidence: f64) -> LayaAnswer {
        LayaAnswer {
            kind: Some("choice".to_string()),
            choice: Some(label.to_string()),
            probabilities: Some(serde_json::json!({ label: confidence })),
            confidence: Some(confidence),
            ..Default::default()
        }
    }

    fn find<'a>(verdicts: &'a [ShadowVerdict], check_name: &str) -> Option<&'a ShadowVerdict> {
        verdicts.iter().find(|v| v.check_name == check_name)
    }

    // ── Fail-open ─────────────────────────────────────────────────────────────────

    #[test]
    fn empty_answers_produce_no_verdicts() {
        assert!(map_answers(&HashMap::new(), true, false, 5).is_empty());
    }

    #[test]
    fn answers_without_usable_values_produce_nothing() {
        let answers = build(&[
            (Q_HALLUCINATION, LayaAnswer::default()),
            (Q_BIAS_PRESENT, LayaAnswer::default()),
            (Q_TOOL_CALL_RISK, LayaAnswer::default()),
        ]);

        let verdicts = map_answers(&answers, true, false, 5);
        assert!(
            verdicts.is_empty(),
            "unusable answers must not invent verdicts"
        );
    }

    #[tokio::test]
    async fn unreachable_judge_fails_open() {
        // Nothing listens on port 1, so this exercises the transport error path.
        let client = LayaClient::new("http://127.0.0.1:1", 500, None, None);
        let state = GovernanceState::new("a response", "a prompt", Some("some context"));

        let verdicts = client.evaluate(&state).await;
        assert!(
            verdicts.is_empty(),
            "an unreachable judge must yield zero verdicts"
        );
    }

    // ── Hallucination / groundedness ──────────────────────────────────────────────

    #[test]
    fn hallucination_escalates_above_threshold() {
        let answers = build(&[(Q_HALLUCINATION, ab(0.95))]);
        let verdicts = map_answers(&answers, true, false, 7);

        let verdict = find(&verdicts, "laya-hallucination").expect("verdict expected");
        assert_eq!(verdict.outcome, Outcome::Escalate);
        assert_eq!(verdict.axis, Axis::Performance);
        assert!((verdict.confidence - 0.95).abs() < 0.001);
    }

    #[test]
    fn hallucination_edits_in_the_middle_band() {
        let answers = build(&[(Q_HALLUCINATION, ab(0.75))]);
        let verdicts = map_answers(&answers, true, false, 7);

        assert_eq!(
            find(&verdicts, "laya-hallucination").unwrap().outcome,
            Outcome::Edit
        );
    }

    #[test]
    fn hallucination_stays_silent_below_the_edit_threshold() {
        let answers = build(&[(Q_HALLUCINATION, ab(0.40))]);
        let verdicts = map_answers(&answers, true, false, 7);

        assert!(find(&verdicts, "laya-hallucination").is_none());
    }

    #[test]
    fn material_severity_upgrades_a_borderline_hallucination() {
        let answers = build(&[
            (Q_HALLUCINATION, ab(0.72)), // would only Edit on its own
            (Q_HALLUCINATION_SEVERITY, rubric(2.0, 0.9)),
        ]);
        let verdicts = map_answers(&answers, true, false, 7);

        assert_eq!(
            find(&verdicts, "laya-hallucination").unwrap().outcome,
            Outcome::Escalate
        );
    }

    #[test]
    fn context_checks_are_skipped_without_context() {
        let answers = build(&[
            (Q_HALLUCINATION, ab(0.99)),
            (Q_GROUNDEDNESS, rubric(0.0, 0.9)),
        ]);
        let verdicts = map_answers(&answers, false, false, 7);

        assert!(find(&verdicts, "laya-hallucination").is_none());
        assert!(find(&verdicts, "laya-groundedness").is_none());
    }

    #[test]
    fn groundedness_polarity_is_inverted() {
        let unsupported = build(&[(Q_GROUNDEDNESS, rubric(0.0, 0.9))]);
        let verdicts = map_answers(&unsupported, true, false, 7);
        assert_eq!(
            find(&verdicts, "laya-groundedness").unwrap().outcome,
            Outcome::Escalate
        );

        let well_supported = build(&[(Q_GROUNDEDNESS, rubric(2.0, 0.9))]);
        let verdicts = map_answers(&well_supported, true, false, 7);
        assert!(find(&verdicts, "laya-groundedness").is_none());
    }

    // ── Injection ─────────────────────────────────────────────────────────────────

    #[test]
    fn injection_escalates_and_reports_the_family() {
        let answers = build(&[
            (Q_INJECTION_ATTEMPT, ab(0.94)),
            (Q_INJECTION_FAMILY, category("role_hijack", 0.8)),
        ]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-prompt-injection").expect("verdict expected");
        assert_eq!(verdict.outcome, Outcome::Escalate);
        assert_eq!(verdict.axis, Axis::Responsibility);
        assert!(
            verdict.reason.contains("role_hijack"),
            "reason: {}",
            verdict.reason
        );
    }

    #[test]
    fn other_category_is_not_reported_as_a_finding_detail() {
        let answers = build(&[
            (Q_INJECTION_ATTEMPT, ab(0.94)),
            (Q_INJECTION_FAMILY, category("other", 0.9)),
        ]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-prompt-injection").unwrap();
        assert!(
            !verdict.reason.contains("family"),
            "reason: {}",
            verdict.reason
        );
    }

    // ── Tool use ──────────────────────────────────────────────────────────────────

    #[test]
    fn destructive_tool_action_escalates() {
        let answers = build(&[(Q_TOOL_CALL_RISK, category("destructive", 0.91))]);
        let verdicts = map_answers(&answers, false, false, 7);

        assert_eq!(
            find(&verdicts, "laya-tool-use").unwrap().outcome,
            Outcome::Escalate
        );
    }

    #[test]
    fn medium_tool_action_edits_and_low_stays_silent() {
        let medium = build(&[(Q_TOOL_CALL_RISK, category("medium", 0.8))]);
        assert_eq!(
            find(&map_answers(&medium, false, false, 7), "laya-tool-use")
                .unwrap()
                .outcome,
            Outcome::Edit
        );

        let low = build(&[(Q_TOOL_CALL_RISK, category("low", 0.95))]);
        assert!(find(&map_answers(&low, false, false, 7), "laya-tool-use").is_none());
    }

    // ── Bias / toxicity / PII ─────────────────────────────────────────────────────

    #[test]
    fn bias_is_a_responsibility_finding_with_a_category() {
        let answers = build(&[
            (Q_BIAS_PRESENT, ab(0.93)),
            (Q_BIAS_CATEGORY, category("gender", 0.7)),
        ]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-bias").unwrap();
        assert_eq!(verdict.axis, Axis::Responsibility);
        assert_eq!(verdict.outcome, Outcome::Escalate);
        assert!(
            verdict.reason.contains("gender"),
            "reason: {}",
            verdict.reason
        );
    }

    #[test]
    fn only_severe_toxicity_acts() {
        let mild = build(&[(Q_TOXICITY_SEVERITY, rubric(1.0, 0.9))]);
        assert!(find(&map_answers(&mild, false, false, 7), "laya-toxicity").is_none());

        let severe = build(&[(Q_TOXICITY_SEVERITY, rubric(2.0, 0.9))]);
        assert_eq!(
            find(&map_answers(&severe, false, false, 7), "laya-toxicity")
                .unwrap()
                .outcome,
            Outcome::Escalate
        );
    }

    #[test]
    fn reidentification_is_a_responsibility_finding() {
        let answers = build(&[
            (Q_IS_REIDENTIFIABLE, ab(0.88)),
            (Q_REID_TYPE, category("quasi", 0.6)),
        ]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-semantic-pii").unwrap();
        assert_eq!(verdict.axis, Axis::Responsibility);
        assert!(
            verdict.reason.contains("quasi"),
            "reason: {}",
            verdict.reason
        );
    }

    // ── Verbosity ─────────────────────────────────────────────────────────────────

    #[test]
    fn verbosity_is_capped_at_edit() {
        let answers = build(&[(Q_FILLER_RATIO, rubric(2.0, 0.95))]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-verbosity").unwrap();
        assert_eq!(verdict.outcome, Outcome::Edit, "padding must not escalate");
        assert_eq!(verdict.axis, Axis::Cost);
    }

    // ── Fallbacks and flags ───────────────────────────────────────────────────────

    #[test]
    fn probability_falls_back_to_confidence_without_a_distribution() {
        let answer = LayaAnswer {
            kind: Some("choice".to_string()),
            choice: Some("B".to_string()),
            confidence: Some(0.93),
            ..Default::default()
        };

        let answers = build(&[(Q_BIAS_PRESENT, answer)]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-bias").unwrap();
        assert!((verdict.confidence - 0.93).abs() < 0.001);
    }

    #[test]
    fn truncated_input_is_disclosed_in_the_reason() {
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.95))]);
        let verdicts = map_answers(&answers, false, true, 7);

        let verdict = find(&verdicts, "laya-bias").unwrap();
        assert!(
            verdict.reason.contains("truncated"),
            "reason: {}",
            verdict.reason
        );
    }

    #[test]
    fn every_emitted_verdict_carries_the_laya_prefix() {
        let answers = build(&[
            (Q_HALLUCINATION, ab(0.95)),
            (Q_GROUNDEDNESS, rubric(0.0, 0.9)),
            (Q_INJECTION_ATTEMPT, ab(0.95)),
            (Q_TOOL_CALL_RISK, category("destructive", 0.9)),
            (Q_BIAS_PRESENT, ab(0.95)),
            (Q_TOXICITY_SEVERITY, rubric(2.0, 0.9)),
            (Q_IS_REIDENTIFIABLE, ab(0.95)),
            (Q_FILLER_RATIO, rubric(2.0, 0.9)),
        ]);

        let verdicts = map_answers(&answers, true, false, 7);
        assert_eq!(verdicts.len(), 8, "one verdict per firing check");

        for verdict in &verdicts {
            assert!(
                verdict.check_name.starts_with("laya-"),
                "unprefixed check name would collide with existing checks: {}",
                verdict.check_name
            );
        }
    }

    // ── Evidence channel (sub-threshold readings for fusion) ──────────────────────

    #[test]
    fn sub_threshold_bias_is_published_as_evidence_only() {
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.55))]);
        let verdicts = map_answers(&answers, false, false, 7);

        let verdict = find(&verdicts, "laya-bias-evidence").expect("evidence expected");
        assert_eq!(
            verdict.outcome,
            Outcome::Pass,
            "evidence must never be actionable on its own"
        );
        assert!((verdict.confidence - 0.55).abs() < 0.001);
        assert!(verdict.reason.contains("no action"));
        // The actionable check name stays absent, so nothing else reacts to it.
        assert!(find(&verdicts, "laya-bias").is_none());
    }

    #[test]
    fn evidence_is_not_emitted_below_the_evidence_floor() {
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.44))]);
        let verdicts = map_answers(&answers, false, false, 7);

        assert!(find(&verdicts, "laya-bias-evidence").is_none());
        assert!(verdicts.is_empty());
    }

    #[test]
    fn evidence_is_superseded_once_the_reading_acts() {
        // At/above the edit threshold the actionable verdict replaces the evidence one.
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.72))]);
        let verdicts = map_answers(&answers, false, false, 7);

        assert!(find(&verdicts, "laya-bias-evidence").is_none());
        assert_eq!(find(&verdicts, "laya-bias").unwrap().outcome, Outcome::Edit);
    }

    #[test]
    fn low_stakes_and_derived_checks_never_emit_evidence() {
        // Verbosity is a cost signal and tool-use is derived from the fast path; neither
        // should add sub-threshold noise to the decision path.
        let answers = build(&[
            (Q_FILLER_RATIO, rubric(1.0, 0.9)),
            (Q_TOOL_CALL_RISK, category("low", 0.6)),
        ]);
        let verdicts = map_answers(&answers, false, false, 7);

        assert!(verdicts.is_empty(), "unexpected verdicts: {verdicts:?}");
    }

    #[test]
    fn evidence_requires_context_for_context_checks() {
        let answers = build(&[(Q_HALLUCINATION, ab(0.55))]);

        assert!(find(&map_answers(&answers, false, false, 7), "laya-hallucination-evidence").is_none());
        assert!(find(&map_answers(&answers, true, false, 7), "laya-hallucination-evidence").is_some());
    }

    // ── Calibration ───────────────────────────────────────────────────────────────

    #[test]
    fn calibration_softens_an_over_confident_judge_below_the_escalation_band() {
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.95))]);

        // Raw: 0.95 escalates.
        let raw = map_answers(&answers, false, false, 7);
        assert_eq!(find(&raw, "laya-bias").unwrap().outcome, Outcome::Escalate);

        // A fitted temperature of 2.5 pulls the same reading into the edit band without
        // washing it out below the evidence floor.
        let mut calibration = Calibration::inert();
        calibration.insert_temperature("laya-bias", Primitive::Choice, 2, 2.5);

        let calibrated = map_answers_calibrated(&answers, false, false, 7, &calibration, 1);
        let verdict = find(&calibrated, "laya-bias").unwrap();
        assert_eq!(verdict.outcome, Outcome::Edit);
        assert!(verdict.confidence < 0.95);
    }

    #[test]
    fn inert_calibration_preserves_the_raw_reading_exactly() {
        let answers = build(&[
            (Q_BIAS_PRESENT, ab(0.81)),
            (Q_IS_REIDENTIFIABLE, ab(0.66)),
            (Q_GROUNDEDNESS, rubric(1.0, 0.8)),
        ]);

        let baseline = map_answers(&answers, true, false, 7);
        let inert = map_answers_calibrated(&answers, true, false, 7, &Calibration::inert(), 1);

        assert_eq!(baseline.len(), inert.len());
        for (a, b) in baseline.iter().zip(inert.iter()) {
            assert_eq!(a.check_name, b.check_name);
            assert_eq!(a.outcome, b.outcome);
            assert!((a.confidence - b.confidence).abs() < 1e-6);
            assert_eq!(a.reason, b.reason);
        }
    }

    // ── Chunk-and-max-pool ────────────────────────────────────────────────────────

    #[test]
    fn max_pool_keeps_the_riskiest_window() {
        let windows = vec![
            build(&[
                (Q_BIAS_PRESENT, ab(0.30)),
                (Q_BIAS_CATEGORY, category("other", 0.9)),
            ]),
            build(&[
                (Q_BIAS_PRESENT, ab(0.96)),
                (Q_BIAS_CATEGORY, category("religion", 0.7)),
            ]),
            build(&[(Q_BIAS_PRESENT, ab(0.50))]),
        ];

        let merged = merge_answers(&windows, &Calibration::inert());
        let verdicts = map_answers(&merged, false, false, 7);

        let verdict = find(&verdicts, "laya-bias").unwrap();
        assert_eq!(verdict.outcome, Outcome::Escalate);
        assert!((verdict.confidence - 0.96).abs() < 0.001);
        // The category comes from the same window that produced the risk, not another one.
        assert!(verdict.reason.contains("religion"), "reason: {}", verdict.reason);
    }

    #[test]
    fn max_pool_is_inverted_for_groundedness() {
        // Groundedness risk is "poorly supported", so the pooled window is the WORST
        // support (rubric level 0), not the best.
        let windows = vec![
            build(&[(Q_GROUNDEDNESS, rubric(2.0, 0.9))]),
            build(&[(Q_GROUNDEDNESS, rubric(0.0, 0.9))]),
        ];

        let merged = merge_answers(&windows, &Calibration::inert());
        let verdicts = map_answers(&merged, true, false, 7);

        assert_eq!(
            find(&verdicts, "laya-groundedness").unwrap().outcome,
            Outcome::Escalate
        );
    }

    #[test]
    fn max_pool_picks_the_highest_tool_risk_across_windows() {
        let windows = vec![
            build(&[(Q_TOOL_CALL_RISK, category("low", 0.9))]),
            build(&[(Q_TOOL_CALL_RISK, category("destructive", 0.8))]),
        ];

        let merged = merge_answers(&windows, &Calibration::inert());
        let verdicts = map_answers(&merged, false, false, 7);

        let verdict = find(&verdicts, "laya-tool-use").unwrap();
        assert_eq!(verdict.outcome, Outcome::Escalate);
        assert!(verdict.reason.contains("destructive"), "reason: {}", verdict.reason);
    }

    #[test]
    fn max_pool_ignores_windows_without_an_answer() {
        let windows = vec![
            HashMap::new(),
            build(&[(Q_IS_REIDENTIFIABLE, ab(0.93))]),
        ];

        let merged = merge_answers(&windows, &Calibration::inert());
        assert!(merged.contains_key(Q_IS_REIDENTIFIABLE));
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn max_pool_of_nothing_is_empty() {
        assert!(merge_answers(&[], &Calibration::inert()).is_empty());
        assert!(merge_answers(&[HashMap::new()], &Calibration::inert()).is_empty());
    }

    #[test]
    fn window_count_is_disclosed_in_the_reason() {
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.95))]);
        let verdicts = map_answers_calibrated(&answers, false, false, 7, &Calibration::inert(), 4);

        let verdict = find(&verdicts, "laya-bias").unwrap();
        assert!(
            verdict.reason.contains("4 windows max-pooled"),
            "reason: {}",
            verdict.reason
        );
    }

    #[test]
    fn a_single_window_does_not_mention_pooling() {
        let answers = build(&[(Q_BIAS_PRESENT, ab(0.95))]);
        let verdicts = map_answers_calibrated(&answers, false, false, 7, &Calibration::inert(), 1);

        let verdict = find(&verdicts, "laya-bias").unwrap();
        assert!(!verdict.reason.contains("windows"), "reason: {}", verdict.reason);
    }

    // ── Scale table integrity ─────────────────────────────────────────────────────

    #[test]
    fn question_scales_cover_every_emitted_check() {
        let questions = build_questions(true);
        let emitted = [
            Q_HALLUCINATION,
            Q_GROUNDEDNESS,
            Q_INJECTION_ATTEMPT,
            Q_TOOL_CALL_RISK,
            Q_BIAS_PRESENT,
            Q_TOXICITY_SEVERITY,
            Q_IS_REIDENTIFIABLE,
            Q_FILLER_RATIO,
        ];

        for key in emitted {
            assert!(
                questions.get(key).is_some(),
                "{key} is mapped but not asked by the schema"
            );
            let scale = QUESTION_SCALES
                .iter()
                .find(|s| s.key == key)
                .unwrap_or_else(|| panic!("{key} has no QuestionScale"));
            assert!(
                scale.detector.starts_with("laya-"),
                "{key} detector must keep the laya- prefix so it is a distinct detector"
            );
        }

        // And no scale points at a question the schema does not ask.
        for scale in QUESTION_SCALES {
            assert!(
                questions.get(scale.key).is_some(),
                "scale for {} has no matching question",
                scale.key
            );
        }
    }

    #[test]
    fn every_context_gated_scale_is_gated_by_the_schema_too() {
        let without_context = build_questions(false);
        for scale in QUESTION_SCALES.iter().filter(|s| s.context_required) {
            assert!(
                without_context.get(scale.key).is_none(),
                "{} is context-gated in the scale table but asked without context",
                scale.key
            );
        }
    }

    #[test]
    fn detail_keys_are_asked_by_the_schema() {
        let questions = build_questions(true);
        for scale in QUESTION_SCALES {
            if let Some(detail) = detail_key_for(scale.key) {
                assert!(
                    questions.get(detail).is_some(),
                    "{detail} (detail of {}) is not in the schema",
                    scale.key
                );
            }
        }
    }

    #[test]
    fn evidence_suffix_never_collides_with_an_actionable_check_name() {
        for scale in QUESTION_SCALES {
            assert!(!scale.detector.ends_with(EVIDENCE_SUFFIX));
            assert!(
                !scale.detector.contains(EVIDENCE_SUFFIX),
                "{} would be ambiguous in the evidence channel",
                scale.detector
            );
        }
    }
}
