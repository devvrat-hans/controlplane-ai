//! Cross-crate integration: real judge output flowing into the real decision fusion.
//!
//! `shadow-analysis` and `decision` never share a data structure — per `AGENTS.md` they
//! communicate over events — so their coupling is a pair of **string conventions** baked
//! into the emitted `check_name`s:
//!
//! - judge verdicts are prefixed `laya-` ([`JUDGE_DETECTOR_PREFIX`]),
//! - sub-threshold evidence readings carry the `-evidence` suffix ([`EVIDENCE_SUFFIX`]).
//!
//! Until now those literals were only *pinned* by a comment and a local unit test inside
//! `decision`. These tests close the loop: they run the **real client** against a fake
//! `laya-serve`, feed the **real verdicts** into the **real fusion**, and fail if the two
//! crates ever drift apart. That is the failure mode a comment cannot catch.
//!
//! See `docs/analysis/laya-integration-plan.md` §5.2 (fusion), §5.6 (disagreement) and §7
//! (contract compliance).
//!
//! Run with: `cargo test -p controlplane-gateway --test hybrid_pipeline_test`

mod common;

use common::{ab, all_firing, FakeLaya, Plan};
use serde_json::json;
use uuid::Uuid;

use controlplane_common::models::Verdict;
use controlplane_common::types::{Axis, Outcome, Path as VerdictPath};
use controlplane_decision::{
    base_detector, is_judge_detector, FusionConfig, VerdictAggregator, EVIDENCE_SUFFIX,
    JUDGE_DETECTOR_PREFIX,
};
use controlplane_shadow_analysis::laya_client::EVIDENCE_SUFFIX as SHADOW_EVIDENCE_SUFFIX;
use controlplane_shadow_analysis::{GovernanceState, LayaClient, ShadowVerdict};

/// The shadow path's verdict shape, converted to the event-carried `Verdict` the decision
/// engine consumes — exactly what the worker does before publishing.
fn to_verdict(shadow: &ShadowVerdict) -> Verdict {
    Verdict::new(
        Uuid::nil(),
        shadow.axis,
        VerdictPath::Shadow,
        shadow.outcome,
        shadow.confidence,
        shadow.reason.clone(),
        shadow.check_name.clone(),
    )
    .with_duration(shadow.duration_ms as i32)
}

/// Run the real client and return the real verdicts it emits (no retrieval context, so
/// the two context-gated checks are correctly skipped).
async fn judge_verdicts(answer: serde_json::Value) -> Vec<ShadowVerdict> {
    let server = FakeLaya::start(vec![], Plan::Answers(answer)).await;
    LayaClient::new(&server.base_url(), 5_000, None, None)
        .evaluate(&GovernanceState::new("response text", "prompt text", None))
        .await
}

/// As above, but with a retrieval context so the hallucination and groundedness
/// questions are asked too.
async fn judge_verdicts_with_context(answer: serde_json::Value) -> Vec<ShadowVerdict> {
    let server = FakeLaya::start(vec![], Plan::Answers(answer)).await;
    LayaClient::new(&server.base_url(), 5_000, None, None)
        .evaluate(&GovernanceState::new(
            "response text",
            "prompt text",
            Some("retrieved context"),
        ))
        .await
}

/// A fusion config that fits `detectors` with `weight`.
fn fitted(detectors: &[(&str, f64)]) -> FusionConfig {
    let mut config = FusionConfig::default();
    for (detector, weight) in detectors {
        config.weights.insert((*detector).to_string(), *weight);
    }
    config
}

fn fuse(verdicts: &[Verdict], config: &FusionConfig) -> controlplane_decision::FusionResult {
    VerdictAggregator::new()
        .fuse_evidence(verdicts, config)
        .expect("the fusion must engage once a detector is fitted")
}

// ─── The cross-crate string contract ─────────────────────────────────────────────

#[test]
fn the_evidence_suffix_literal_agrees_across_the_crate_boundary() {
    // Previously only asserted against a local copy inside `decision`. Now the two
    // crates' constants are compared directly, so the pair cannot silently diverge.
    assert_eq!(EVIDENCE_SUFFIX, SHADOW_EVIDENCE_SUFFIX);
    assert_eq!(EVIDENCE_SUFFIX, "-evidence");
    assert_eq!(JUDGE_DETECTOR_PREFIX, "laya-");
}

#[tokio::test]
async fn every_judge_check_name_the_client_emits_is_recognised_by_the_decision_engine() {
    let verdicts = judge_verdicts_with_context(all_firing(0.95)).await;
    assert_eq!(
        verdicts.len(),
        8,
        "every question fires when a context is present: {:?}",
        verdicts.iter().map(|v| &v.check_name).collect::<Vec<_>>()
    );

    for verdict in &verdicts {
        let name = &verdict.check_name;
        assert!(
            name.starts_with(JUDGE_DETECTOR_PREFIX),
            "{name} would be aggregated as a heuristic"
        );
        assert!(
            is_judge_detector(name),
            "the decision engine must classify {name} as the calibrated judge"
        );
        assert_eq!(
            base_detector(name),
            name,
            "{name} is an actionable verdict, so it must have no suffix to strip"
        );
    }
}

#[tokio::test]
async fn an_evidence_reading_is_attributed_to_its_own_detector_name() {
    let verdicts = judge_verdicts(json!({ "bias_present": ab(0.55) })).await;
    let name = &verdicts[0].check_name;

    assert_eq!(name, &format!("laya-bias{EVIDENCE_SUFFIX}"));
    assert!(
        is_judge_detector(name),
        "an evidence reading must still be attributed to the judge"
    );
    assert_eq!(
        base_detector(name),
        "laya-bias",
        "stripping the suffix must recover the fitted detector key"
    );
}

// ─── End-to-end: client → verdicts → fusion ──────────────────────────────────────

#[tokio::test]
async fn a_confident_judge_verdict_escalates_through_the_real_fusion() {
    let verdicts = judge_verdicts(json!({ "bias_present": ab(0.95) })).await;
    let verdicts: Vec<Verdict> = verdicts.iter().map(to_verdict).collect();

    let result = fuse(&verdicts, &fitted(&[("laya-bias", 1.0)]));

    assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
    assert!(result.axes[0].detectors[0].calibrated);
    assert_eq!(result.axes[0].detectors[0].weight, Some(1.0));
}

#[tokio::test]
async fn a_mid_band_judge_verdict_edits_through_the_real_fusion() {
    let verdicts = judge_verdicts(json!({ "bias_present": ab(0.75) })).await;
    let verdicts: Vec<Verdict> = verdicts.iter().map(to_verdict).collect();

    let result = fuse(&verdicts, &fitted(&[("laya-bias", 1.0)]));

    assert_eq!(result.aggregation.final_outcome, Outcome::Edit);
    assert!((result.axes[0].p - 0.75).abs() < 0.001);
}

#[tokio::test]
async fn a_judge_verdict_can_never_block_alone() {
    // Only the fast path may block synchronously; a shadow-path judge escalates at most.
    let verdicts = judge_verdicts_with_context(all_firing(0.99)).await;
    let verdicts: Vec<Verdict> = verdicts.iter().map(to_verdict).collect();

    let mut config = fitted(&[
        ("laya-bias", 1.0),
        ("laya-prompt-injection", 1.0),
        ("laya-toxicity", 1.0),
        ("laya-semantic-pii", 1.0),
        ("laya-hallucination", 1.0),
        ("laya-groundedness", 1.0),
    ]);
    config.calibration_version = Some(1);

    let result = fuse(&verdicts, &config);

    assert_eq!(
        result.aggregation.final_outcome,
        Outcome::Escalate,
        "a judge finding escalates; blocking stays a fast-path decision"
    );
}

#[tokio::test]
async fn the_evidence_band_alone_cannot_act_through_the_fusion() {
    // 0.55 is real signal for the fusion to weigh, but on its own it must not escalate:
    // the judge is below its escalation confidence and nothing corroborates it.
    let verdicts = judge_verdicts(json!({ "bias_present": ab(0.55) })).await;
    let verdicts: Vec<Verdict> = verdicts.iter().map(to_verdict).collect();

    let result = fuse(&verdicts, &fitted(&[("laya-bias", 1.0)]));

    assert!(result.axes[0].detectors[0].weight.is_some());
    assert_eq!(result.axes[0].outcome, Outcome::Pass);
    assert_eq!(result.aggregation.final_outcome, Outcome::Pass);
}

#[tokio::test]
async fn judge_and_heuristic_disagreement_routes_the_real_traffic_to_a_human() {
    // The precise case plan §5.6 is about: the deterministic regex fires hard while the
    // calibrated judge reads the same response as nearly clean. A human should look.
    let mut verdicts: Vec<Verdict> = judge_verdicts(json!({ "bias_present": ab(0.48) }))
        .await
        .iter()
        .map(to_verdict)
        .collect();

    // The in-process heuristic's own verdict, as the worker would have emitted it.
    verdicts.push(Verdict::new(
        Uuid::nil(),
        Axis::Responsibility,
        VerdictPath::Shadow,
        Outcome::Escalate,
        0.95,
        "matched 'ignore previous instructions'",
        "prompt_injection",
    ));

    // Only the judge is inside the fit; the heuristic keeps its existing authority.
    let result = fuse(&verdicts, &fitted(&[("laya-bias", 0.5)]));

    assert!(result.disagreement, "{result:?}");
    assert_eq!(result.aggregation.final_outcome, Outcome::Escalate);
    let reason = result.disagreement_reason.expect("a reason is recorded");
    assert!(reason.contains("disagreement"), "reason: {reason}");
    assert!(result.aggregation.primary_reason.contains("disagreement"));
    assert!(!result.aggregation.contributing_ids.is_empty());
}

// ─── The un-fitted default must not change behaviour ─────────────────────────────

#[tokio::test]
async fn with_no_fit_the_decision_engine_keeps_its_previous_aggregator() {
    let verdicts = judge_verdicts(all_firing(0.95)).await;
    let verdicts: Vec<Verdict> = verdicts.iter().map(to_verdict).collect();

    let aggregator = VerdictAggregator::new();

    // No fitted weights -> the fusion declines to engage, which is the signal for the
    // caller to use the pre-fusion aggregator (plan §7 fail-open matrix, row 1).
    assert!(aggregator
        .fuse_evidence(&verdicts, &FusionConfig::default())
        .is_none());

    let previous = aggregator.aggregate_with_reasoning(&verdicts);
    assert_eq!(previous.final_outcome, Outcome::Escalate);
}

#[tokio::test]
async fn a_fitted_judge_does_not_down_weight_an_unfitted_heuristic() {
    // The safety property from the plan's as-built notes: fitting only the judge must
    // leave every other detector with exactly the authority it has today.
    let mut verdicts: Vec<Verdict> = judge_verdicts(json!({ "bias_present": ab(0.02) }))
        .await
        .iter()
        .map(to_verdict)
        .collect();

    // A clean judge reading produces no verdict at all, so add the heuristic on its own.
    verdicts.push(Verdict::new(
        Uuid::nil(),
        Axis::Responsibility,
        VerdictPath::Shadow,
        Outcome::Escalate,
        0.88,
        "re-identification risk: 3 quasi-identifier categories detected",
        "semantic_pii",
    ));

    let result = fuse(&verdicts, &fitted(&[("laya-bias", 0.8)]));

    assert_eq!(
        result.axes[0].detectors.len(),
        1,
        "only the heuristic remains"
    );
    assert_eq!(result.axes[0].detectors[0].weight, None, "outside the fit");
    assert_eq!(
        result.aggregation.final_outcome,
        Outcome::Escalate,
        "an unfitted detector must keep its existing authority"
    );
}
