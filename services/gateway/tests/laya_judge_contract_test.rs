//! Laya judge integration tests — the real client against a real socket.
//!
//! These exercise the seam the in-crate unit tests cannot reach: the HTTP contract between
//! [`LayaClient`] and a `laya-serve`-compatible endpoint, plus fail-open across the wire.
//! `docs/analysis/laya-integration-plan.md` §7 requires that a judge which is down, slow,
//! or sending garbage changes **nothing** about the outcome — absence of a verdict is a
//! pass.
//!
//! The fake server (`tests/common/mod.rs`) implements the Jev-compatible
//! `POST /v1/systemone` request/response shape, so these tests fail if the client's wire
//! format ever drifts from what a real `laya-serve` accepts.
//!
//! Run with: `cargo test -p controlplane-gateway --test laya_judge_contract_test`

mod common;

use std::time::Duration;

use common::{ab, all_clean, all_firing, category, score, FakeLaya, Plan};
use serde_json::json;

use controlplane_common::types::{Axis, Outcome};
use controlplane_shadow_analysis::laya_client::{EDIT_THRESHOLD, EVIDENCE_SUFFIX};
use controlplane_shadow_analysis::{GovernanceState, LayaClient, ShadowVerdict};

const Q_HALLUCINATION: &str = "hallucination";
const Q_GROUNDEDNESS: &str = "groundedness";
const Q_INJECTION: &str = "injection_attempt";
const Q_TOOL_RISK: &str = "tool_call_risk";

/// A client pointed at the fake server, with no model override and no API key.
fn client(server: &FakeLaya) -> LayaClient {
    LayaClient::new(&server.base_url(), 5_000, None, None)
}

fn find<'a>(verdicts: &'a [ShadowVerdict], check_name: &str) -> Option<&'a ShadowVerdict> {
    verdicts.iter().find(|v| v.check_name == check_name)
}

fn names(verdicts: &[ShadowVerdict]) -> Vec<String> {
    let mut names: Vec<String> = verdicts.iter().map(|v| v.check_name.clone()).collect();
    names.sort();
    names
}

// ─── Request shape (plan §8) ─────────────────────────────────────────────────────

#[tokio::test]
async fn a_short_response_is_exactly_one_call_with_the_jev_request_shape() {
    let server = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    let state = GovernanceState::new("a short response", "a short prompt", None);

    client(&server).evaluate(&state).await;

    assert_eq!(
        server.request_count(),
        1,
        "Laya answers every question in one forward pass, so a short call is one HTTP call"
    );

    let request = &server.requests()[0];
    assert_eq!(request.response_text(), "a short response");
    assert_eq!(request.prompt_text(), "a short prompt");

    // The top-level body is exactly the contract: state + questions.
    let top_level: Vec<String> = request
        .body
        .as_object()
        .expect("body is an object")
        .keys()
        .cloned()
        .collect();
    assert!(top_level.contains(&"state".to_string()));
    assert!(top_level.contains(&"questions".to_string()));

    // Without a model override the Router is left to choose the checkpoint.
    assert!(
        request.body.get("model").is_none(),
        "model must be omitted so Laya's Router picks by language"
    );
}

#[tokio::test]
async fn context_is_omitted_from_the_state_when_there_is_none() {
    let server = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    let state = GovernanceState::new("response", "prompt", None);

    client(&server).evaluate(&state).await;

    let request = &server.requests()[0];
    assert!(request.context_text().is_none());
    assert!(
        request.body["state"].get("context").is_none(),
        "a null context key would pad the state and cost accuracy"
    );
}

#[tokio::test]
async fn context_questions_are_asked_only_when_a_context_exists() {
    let without = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    client(&without)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    let keys = without.requests()[0].question_keys();
    assert!(
        !keys.contains(&Q_HALLUCINATION.to_string()),
        "hallucination is unanswerable without ground truth: {keys:?}"
    );
    assert!(!keys.contains(&Q_GROUNDEDNESS.to_string()));
    assert!(
        keys.contains(&Q_INJECTION.to_string()),
        "unconditional checks must still be asked: {keys:?}"
    );

    let with = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    client(&with)
        .evaluate(&GovernanceState::new("r", "p", Some("retrieved context")))
        .await;

    let request = &with.requests()[0];
    let keys = request.question_keys();
    assert!(keys.contains(&Q_HALLUCINATION.to_string()));
    assert!(keys.contains(&Q_GROUNDEDNESS.to_string()));
    assert_eq!(request.context_text(), Some("retrieved context"));
}

#[tokio::test]
async fn question_payloads_follow_the_typed_schema() {
    let server = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    client(&server)
        .evaluate(&GovernanceState::new("r", "p", Some("ctx")))
        .await;

    let request = &server.requests()[0];

    // A yes/no question is a two-option `choice` with neutral A/B keys — never `noul`.
    let hallucination = &request.questions()[Q_HALLUCINATION];
    assert_eq!(hallucination["type"], "choice");
    assert!(hallucination["criteria"].get("A").is_some());
    assert!(hallucination["criteria"].get("B").is_some());
    assert!(
        hallucination["criteria"].get("true").is_none(),
        "the noul A/B form avoids the label-following bug"
    );

    // A severity question is a `score` rubric with the levels as a criteria array.
    let groundedness = &request.questions()[Q_GROUNDEDNESS];
    assert_eq!(groundedness["type"], "score");
    assert_eq!(
        groundedness["criteria"]
            .as_array()
            .expect("rubric levels")
            .len(),
        3
    );

    // Every category question must offer an explicit way to decline.
    for key in [
        "bias_category",
        "injection_family",
        "reid_type",
        Q_TOOL_RISK,
    ] {
        let criteria = &request.questions()[key]["criteria"];
        assert!(
            criteria.get("other").is_some(),
            "{key} needs an 'other' escape hatch"
        );
        assert!(criteria.as_object().expect("options").len() <= 20);
    }

    // No question may use the `noul` primitive (plan §8).
    for (key, value) in request.questions().as_object().expect("questions object") {
        assert_ne!(value["type"], "noul", "{key} should use a typed primitive");
    }
}

#[tokio::test]
async fn the_model_override_is_sent_when_configured() {
    let server = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    let client = LayaClient::new(
        &server.base_url(),
        5_000,
        Some("typed-decisions".to_string()),
        None,
    );

    client.evaluate(&GovernanceState::new("r", "p", None)).await;

    assert_eq!(server.requests()[0].body["model"], "typed-decisions");
}

#[tokio::test]
async fn a_bearer_token_is_sent_only_when_one_is_configured() {
    let secured = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    LayaClient::new(&secured.base_url(), 5_000, None, Some("s3cret".to_string()))
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert_eq!(
        secured.requests()[0].authorization.as_deref(),
        Some("Bearer s3cret")
    );

    let open = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    client(&open)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(
        open.requests()[0].authorization.is_none(),
        "an unauthenticated laya-serve must not receive an Authorization header"
    );
}

// ─── Response mapping (plan §9) ──────────────────────────────────────────────────

#[tokio::test]
async fn answers_are_mapped_to_prefixed_verdicts_with_the_expected_axes() {
    let server = FakeLaya::start(vec![], Plan::Answers(all_firing(0.95))).await;
    let state = GovernanceState::new("response", "prompt", Some("context"));

    let verdicts = client(&server).evaluate(&state).await;

    assert_eq!(
        names(&verdicts),
        vec![
            "laya-bias",
            "laya-groundedness",
            "laya-hallucination",
            "laya-prompt-injection",
            "laya-semantic-pii",
            "laya-tool-use",
            "laya-toxicity",
            "laya-verbosity",
        ]
    );

    // Axis assignment is fixed by the schema so the dashboard breakdown stays coherent.
    assert_eq!(
        find(&verdicts, "laya-bias").unwrap().axis,
        Axis::Responsibility
    );
    assert_eq!(
        find(&verdicts, "laya-prompt-injection").unwrap().axis,
        Axis::Responsibility
    );
    assert_eq!(
        find(&verdicts, "laya-hallucination").unwrap().axis,
        Axis::Performance
    );
    assert_eq!(find(&verdicts, "laya-verbosity").unwrap().axis, Axis::Cost);
}

#[tokio::test]
async fn a_high_reading_escalates_and_a_clean_call_says_nothing() {
    let dirty = FakeLaya::start(vec![], Plan::Answers(all_firing(0.95))).await;
    let verdicts = client(&dirty)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;
    assert_eq!(
        find(&verdicts, "laya-bias").unwrap().outcome,
        Outcome::Escalate
    );

    let clean = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;
    let verdicts = client(&clean)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(
        verdicts.is_empty(),
        "a clean reading must produce no verdict at all — absence means pass: {:?}",
        names(&verdicts)
    );
}

#[tokio::test]
async fn sub_threshold_readings_are_published_as_evidence_and_never_as_action() {
    let server = FakeLaya::start(vec![], Plan::Answers(json!({ "bias_present": ab(0.55) }))).await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    let evidence = find(&verdicts, &format!("laya-bias{EVIDENCE_SUFFIX}"))
        .expect("a 0.55 reading belongs in the evidence band");
    assert_eq!(evidence.outcome, Outcome::Pass);
    assert!((evidence.confidence - 0.55).abs() < 0.001);
    assert!(
        find(&verdicts, "laya-bias").is_none(),
        "evidence must not also emit an actionable verdict"
    );

    // Just below the floor the reading is noise and is dropped entirely.
    let noisy = FakeLaya::start(
        vec![],
        Plan::Answers(json!({ "bias_present": ab(EDIT_THRESHOLD - 0.26) })),
    )
    .await;
    assert!(client(&noisy)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await
        .is_empty());
}

#[tokio::test]
async fn only_severe_toxicity_acts_because_the_specialist_model_owns_the_middle() {
    let mild = FakeLaya::start(
        vec![],
        Plan::Answers(json!({ "toxicity_severity": score(1.0, 0.9) })),
    )
    .await;

    let verdicts = client(&mild)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;
    assert!(
        find(&verdicts, "laya-toxicity").is_none(),
        "a mild reading must not act while Toxic-BERT evaluates the middle of the scale"
    );

    let severe = FakeLaya::start(
        vec![],
        Plan::Answers(json!({ "toxicity_severity": score(2.0, 0.95) })),
    )
    .await;
    let verdicts = client(&severe)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;
    assert_eq!(
        find(&verdicts, "laya-toxicity").unwrap().outcome,
        Outcome::Escalate
    );
}

#[tokio::test]
async fn category_answers_carry_the_family_into_the_reason() {
    let server = FakeLaya::start(
        vec![],
        Plan::Answers(json!({
            "injection_attempt": ab(0.94),
            "injection_family": category("prompt_extraction", 0.8),
        })),
    )
    .await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    let verdict = find(&verdicts, "laya-prompt-injection").unwrap();
    assert!(
        verdict.reason.contains("prompt_extraction"),
        "the attack family feeds pattern_promotion: {}",
        verdict.reason
    );
}

#[tokio::test]
async fn a_destructive_tool_action_escalates_while_a_low_one_stays_silent() {
    let destructive = FakeLaya::start(
        vec![],
        Plan::Answers(json!({ "tool_call_risk": category("destructive", 0.93) })),
    )
    .await;

    let verdicts = client(&destructive)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;
    let verdict = find(&verdicts, "laya-tool-use").unwrap();
    assert_eq!(verdict.outcome, Outcome::Escalate);
    assert_eq!(verdict.axis, Axis::Performance);

    let low = FakeLaya::start(
        vec![],
        Plan::Answers(json!({ "tool_call_risk": category("low", 0.95) })),
    )
    .await;
    assert!(
        client(&low)
            .evaluate(&GovernanceState::new("r", "p", None))
            .await
            .is_empty(),
        "grading a benign read must not create a finding"
    );
}

#[tokio::test]
async fn a_choice_answer_without_a_distribution_falls_back_to_its_confidence() {
    // The documented Jev/Laya response form is per-question scalars
    // (`answers[key].noul` / `.score`), so a `choice` reply may carry only the selected
    // option plus its confidence and no full distribution. The mapping must still work.
    let server = FakeLaya::start(
        vec![],
        Plan::Answers(json!({
            "bias_present": { "type": "choice", "choice": "B", "confidence": 0.95 },
        })),
    )
    .await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    let verdict = find(&verdicts, "laya-bias").unwrap();
    assert_eq!(verdict.outcome, Outcome::Escalate);
    assert!((verdict.confidence - 0.95).abs() < 0.001);
}

#[tokio::test]
async fn a_noul_only_answer_is_not_consumed() {
    // Deliberate design decision, pinned here so it stays deliberate: the schema asks
    // critical yes/no questions as two-option `choice` with neutral A/B keys, *not* as
    // the `noul` primitive (plan §8 — on the English checkpoint `noul` can follow its own
    // option labels instead of the state). If a backend answers a question with `noul`
    // anyway, the reading is not interpreted, so it contributes nothing rather than
    // guessing a polarity it cannot know.
    //
    // This is the fail-open contract working as intended, but it is a *silent* no-op: if
    // a future backend returns `noul` by default, the judge would quietly stop
    // contributing. That is the thing to watch when pointing DECISION_JUDGE at a real
    // `laya-serve`.
    let server = FakeLaya::start(
        vec![],
        Plan::Answers(json!({
            "bias_present": { "type": "noul", "noul": 0.95 },
            "injection_attempt": { "type": "noul", "noul": 0.95 },
        })),
    )
    .await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(
        verdicts.is_empty(),
        "an unsupported primitive must yield nothing rather than a guessed reading: {verdicts:?}"
    );
}

// ─── Fail-open across the wire (plan §7) ─────────────────────────────────────────

#[tokio::test]
async fn an_error_status_fails_open() {
    let server = FakeLaya::start(vec![Plan::Status(500)], Plan::Status(500)).await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(
        verdicts.is_empty(),
        "a 5xx must yield zero verdicts, never a guess"
    );
}

#[tokio::test]
async fn malformed_json_fails_open() {
    let server = FakeLaya::start(vec![Plan::Malformed], Plan::Malformed).await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(verdicts.is_empty(), "unparseable payload must not panic");
}

#[tokio::test]
async fn a_timeout_fails_open() {
    let server =
        FakeLaya::start(vec![], Plan::Slow(Duration::from_secs(5), all_firing(0.99))).await;

    // The client's budget is far below the server's delay.
    let verdicts = LayaClient::new(&server.base_url(), 150, None, None)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(
        verdicts.is_empty(),
        "the shadow path must never wait on the judge"
    );
}

#[tokio::test]
async fn an_unreachable_judge_fails_open() {
    // Nothing listens on port 1, so this is a transport error rather than an HTTP one.
    let verdicts = LayaClient::new("http://127.0.0.1:1", 500, None, None)
        .evaluate(&GovernanceState::new("r", "p", None))
        .await;

    assert!(verdicts.is_empty());
}

#[tokio::test]
async fn partial_window_failure_still_uses_the_windows_that_answered() {
    // Window 0 answers, window 1 returns a 500. The judge degrades to what it has
    // instead of discarding a real finding (plan §7 fail-open matrix).
    let long = format!("HEAD{}MIDDLE{}TAIL", "h".repeat(3000), "t".repeat(3000));
    let server = FakeLaya::start(
        vec![
            Plan::Answers(json!({ "bias_present": ab(0.96) })),
            Plan::Status(500),
        ],
        Plan::Status(500),
    )
    .await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new(&long, "p", None))
        .await;

    assert!(server.request_count() >= 2, "the long response was chunked");
    assert_eq!(
        find(&verdicts, "laya-bias").map(|v| v.outcome),
        Some(Outcome::Escalate),
        "the window that answered must still contribute"
    );
}

// ─── Chunk-and-max-pool over the wire (plan §8.1) ────────────────────────────────

#[tokio::test]
async fn a_long_response_is_split_into_multiple_calls_and_discloses_it() {
    let long = "x".repeat(9_000);
    let server = FakeLaya::start(vec![], Plan::Answers(all_firing(0.95))).await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new(&long, "p", None))
        .await;

    assert!(
        server.request_count() > 1,
        "a 9k-char response must be chunked, got {} call(s)",
        server.request_count()
    );

    let verdict = find(&verdicts, "laya-bias").unwrap();
    assert!(
        verdict.reason.contains("truncated"),
        "the reason must disclose truncation: {}",
        verdict.reason
    );
    assert!(
        verdict.reason.contains("windows max-pooled"),
        "the reason must disclose the window count: {}",
        verdict.reason
    );
}

#[tokio::test]
async fn every_window_carries_the_prompt_so_the_judge_always_sees_the_question() {
    let long = "y".repeat(9_000);
    let server = FakeLaya::start(vec![], Plan::Answers(all_clean())).await;

    client(&server)
        .evaluate(&GovernanceState::new(&long, "the original prompt", None))
        .await;

    assert!(server.request_count() > 1);
    for (index, request) in server.requests().iter().enumerate() {
        assert_eq!(
            request.prompt_text(),
            "the original prompt",
            "window {index} lost the prompt"
        );
    }
}

#[tokio::test]
async fn a_finding_only_in_the_elided_middle_is_still_caught() {
    // The marker sits strictly between the head and the tail slices that
    // `truncate_head_tail` keeps, so the single head+tail window cannot see it. Only the
    // overlapping middle windows can — which is the whole point of chunk-and-max-pool.
    let marker = "REIDENTIFIABLE-MARKER";
    let response = format!("{}{}{}", "h".repeat(1200), marker, "t".repeat(1200));

    let server = FakeLaya::start(
        vec![],
        Plan::Conditional {
            needle: marker.to_string(),
            when_present: json!({ "bias_present": ab(0.96) }),
            otherwise: json!({ "bias_present": ab(0.02) }),
        },
    )
    .await;

    let state = GovernanceState::new(&response, "p", None);
    assert!(
        !state.to_json()["response"]
            .as_str()
            .unwrap()
            .contains(marker),
        "precondition: the head+tail window must not contain the marker"
    );

    let verdicts = client(&server).evaluate(&state).await;

    assert!(
        server.request_count() > 1,
        "the response must be chunked for the marker to be reachable"
    );
    assert_eq!(
        find(&verdicts, "laya-bias").map(|v| v.outcome),
        Some(Outcome::Escalate),
        "a finding in the elided middle must surface (max-pool), got {:?}",
        names(&verdicts)
    );
}

#[tokio::test]
async fn max_pooling_prefers_the_riskiest_window_not_the_last_one() {
    // Window 1 is risky, window 2 is clean. Taking the last window would lose the
    // finding entirely.
    let long = format!("{}MID{}", "a".repeat(2500), "b".repeat(2500));
    let server = FakeLaya::start(
        vec![
            Plan::Answers(json!({ "bias_present": ab(0.02) })),
            Plan::Answers(json!({ "bias_present": ab(0.97) })),
        ],
        Plan::Answers(json!({ "bias_present": ab(0.02) })),
    )
    .await;

    let verdicts = client(&server)
        .evaluate(&GovernanceState::new(&long, "p", None))
        .await;

    let verdict = find(&verdicts, "laya-bias").expect("the risky window must win");
    assert_eq!(verdict.outcome, Outcome::Escalate);
    assert!((verdict.confidence - 0.97).abs() < 0.001);
}
