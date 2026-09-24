//! Shared test harness: a fake `laya-serve` listening on a real TCP socket.
//!
//! The judge is a *network* dependency, and the parts of the integration that can
//! silently break are exactly the parts the pure unit tests cannot see:
//!
//! - the Jev-compatible request shape (`state` + `questions`) the real server must accept,
//! - the `answers` envelope it returns,
//! - bearer auth and the `model` override,
//! - the chunk-and-max-pool fan-out over multiple HTTP round-trips,
//! - and fail-open across the wire (5xx, malformed JSON, timeout).
//!
//! So these tests run the **real** [`LayaClient`] against a real socket and assert the
//! bytes on the wire. See `docs/analysis/laya-integration-plan.md` §6 (architecture),
//! §8 (request schema) and §9 (response mapping).
//!
//! [`LayaClient`]: controlplane_shadow_analysis::LayaClient

#![allow(dead_code)]

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{routing::post, Json, Router};
use serde_json::{json, Value};

/// What the fake server does for one `POST /v1/systemone`.
#[derive(Clone)]
pub enum Plan {
    /// 200 with this `answers` object, wrapped in the Jev response envelope.
    Answers(Value),
    /// 200 with the answers chosen by whether the request's `state.response`
    /// contains `needle`.
    ///
    /// This is how a windowed test proves the chunking is real: only the window that
    /// actually carries the marker can produce the finding.
    Conditional {
        needle: String,
        when_present: Value,
        otherwise: Value,
    },
    /// An explicit HTTP status with an empty body.
    Status(u16),
    /// 200 `application/json` whose body is not valid JSON.
    Malformed,
    /// 200 with `answers` after sleeping, to exercise the client timeout.
    Slow(Duration, Value),
}

/// One request as the server actually received it.
#[derive(Clone, Debug)]
pub struct RecordedRequest {
    pub body: Value,
    pub authorization: Option<String>,
}

impl RecordedRequest {
    /// Text of `state.response`, or `""` when the field is absent.
    pub fn response_text(&self) -> &str {
        self.body["state"]["response"].as_str().unwrap_or_default()
    }

    pub fn prompt_text(&self) -> &str {
        self.body["state"]["prompt"].as_str().unwrap_or_default()
    }

    pub fn context_text(&self) -> Option<&str> {
        self.body["state"]["context"].as_str()
    }

    pub fn questions(&self) -> &Value {
        &self.body["questions"]
    }

    /// Question keys in the request, sorted so assertions are order-independent.
    pub fn question_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .questions()
            .as_object()
            .map(|object| object.keys().cloned().collect())
            .unwrap_or_default();
        keys.sort();
        keys
    }
}

struct Inner {
    /// Queued per-request plans; consumed as requests arrive.
    plan: Mutex<VecDeque<Plan>>,
    requests: Mutex<Vec<RecordedRequest>>,
    /// Answers every request once `plan` is exhausted, so a chunking test can reply to
    /// an unbounded number of windows.
    fallback: Plan,
}

/// A `laya-serve` stand-in bound to an ephemeral loopback port.
#[derive(Clone)]
pub struct FakeLaya {
    inner: Arc<Inner>,
    addr: SocketAddr,
}

impl FakeLaya {
    /// Start the server. When the queued `plans` run out, `fallback` answers every
    /// further request.
    pub async fn start(plans: Vec<Plan>, fallback: Plan) -> Self {
        let inner = Arc::new(Inner {
            plan: Mutex::new(plans.into()),
            requests: Mutex::new(Vec::new()),
            fallback,
        });

        let app = Router::new()
            .route("/v1/systemone", post(handle))
            .with_state(Arc::clone(&inner));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind an ephemeral loopback port");
        let addr = listener.local_addr().expect("read the bound address");

        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        Self { inner, addr }
    }

    /// `http://127.0.0.1:<port>` — the `LAYA_URL` the client is pointed at.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.inner
            .requests
            .lock()
            .expect("requests mutex is never poisoned")
            .clone()
    }

    pub fn request_count(&self) -> usize {
        self.inner
            .requests
            .lock()
            .expect("requests mutex is never poisoned")
            .len()
    }
}

async fn handle(
    State(inner): State<Arc<Inner>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let response_text = body["state"]["response"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    inner
        .requests
        .lock()
        .expect("requests mutex is never poisoned")
        .push(RecordedRequest {
            body,
            authorization,
        });

    let planned = {
        let mut queue = inner.plan.lock().expect("plan mutex is never poisoned");
        queue.pop_front()
    };

    match planned {
        Some(plan) => respond(plan, &response_text).await,
        // No queued plan: the request-inspecting fallback is evaluated here, against the
        // body we just received.
        None => respond(inner.fallback.clone(), &response_text).await,
    }
}

async fn respond(plan: Plan, response_text: &str) -> Response {
    match plan {
        Plan::Answers(answers) => envelope(answers),
        Plan::Conditional {
            needle,
            when_present,
            otherwise,
        } => {
            if response_text.contains(&needle) {
                envelope(when_present)
            } else {
                envelope(otherwise)
            }
        }
        Plan::Status(code) => StatusCode::from_u16(code)
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
            .into_response(),
        Plan::Malformed => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            "{{{ this is not json",
        )
            .into_response(),
        Plan::Slow(delay, answers) => {
            tokio::time::sleep(delay).await;
            envelope(answers)
        }
    }
}

/// The Jev-compatible response envelope: `{ "answers": {...}, "model": "..." }`.
fn envelope(answers: Value) -> Response {
    Json(json!({ "answers": answers, "model": "fake-laya" })).into_response()
}

// ─── Answer builders ─────────────────────────────────────────────────────────────
//
// These mirror the payload shapes documented in the plan §8/§9: a `choice` answer
// carries the chosen option plus a distribution; a `score` answer carries an ordinal.

/// A two-option `choice` answer where `B` is the unsafe option.
pub fn ab(probability_positive: f64) -> Value {
    let chosen = if probability_positive >= 0.5 {
        "B"
    } else {
        "A"
    };
    json!({
        "type": "choice",
        "choice": chosen,
        "probabilities": { "A": 1.0 - probability_positive, "B": probability_positive },
        "confidence": probability_positive.max(1.0 - probability_positive),
    })
}

/// A multi-option category `choice` answer.
pub fn category(label: &str, confidence: f64) -> Value {
    json!({
        "type": "choice",
        "choice": label,
        "probabilities": { label: confidence },
        "confidence": confidence,
    })
}

/// A `score` rubric answer.
pub fn score(value: f64, confidence: f64) -> Value {
    json!({ "type": "score", "score": value, "confidence": confidence })
}

/// Every driving question firing at `p` — the "all checks agree" payload.
pub fn all_firing(p: f64) -> Value {
    json!({
        "hallucination": ab(p),
        "groundedness": score(0.0, 0.95),
        "injection_attempt": ab(p),
        "injection_family": category("role_hijack", 0.8),
        "tool_call_risk": category("destructive", 0.9),
        "bias_present": ab(p),
        "bias_category": category("gender", 0.7),
        "toxicity_severity": score(2.0, 0.95),
        "is_reidentifiable": ab(p),
        "reid_type": category("quasi", 0.6),
        "filler_ratio": score(2.0, 0.95),
    })
}

/// Every question reading clean — must produce no actionable verdict.
pub fn all_clean() -> Value {
    json!({
        "hallucination": ab(0.02),
        "groundedness": score(2.0, 0.95),
        "injection_attempt": ab(0.02),
        "tool_call_risk": category("low", 0.9),
        "bias_present": ab(0.02),
        "toxicity_severity": score(0.0, 0.95),
        "is_reidentifiable": ab(0.02),
        "filler_ratio": score(0.0, 0.95),
    })
}
