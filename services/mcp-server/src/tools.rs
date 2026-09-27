//! Tool registry and implementations.
//!
//! Every tool maps 1:1 onto an existing HTTP endpoint; none of them invent an
//! API. The module owns input validation so that path/query injection and
//! payload abuse are rejected before a request is made.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::auth::{Capability, Principal};
use crate::client::{sanitize_response, ControlPlaneClient};
use crate::config::McpConfig;
use crate::error::McpError;
use crate::protocol::ToolDefinition;

pub struct ToolContext<'a> {
    pub client: &'a ControlPlaneClient,
    pub principal: &'a Principal,
    pub correlation_id: String,
    pub config: &'a McpConfig,
}

// ─── Definitions ─────────────────────────────────────────────────────────────

/// The complete advertised tool surface.
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        read_tool("list_apps", "List registered applications with their governance level.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_policy", "Get the active governance policy for an application.", json!({"type":"object","properties":{"app_id":{"type":"string","description":"Application UUID"}},"required":["app_id"],"additionalProperties":false})),
        read_tool("list_profiles", "List regulatory governance profiles and their thresholds.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("list_requests", "List intercepted API calls with optional search and filters.", json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":500},"offset":{"type":"integer","minimum":0},"search":{"type":"string"},"model":{"type":"string"},"outcome":{"type":"string","enum":["pass","edit","block","escalate"]},"app_id":{"type":"string"}},"additionalProperties":false})),
        read_tool("get_request", "Get one intercepted call with its verdicts, audit records and escalation.", json!({"type":"object","properties":{"call_id":{"type":"string","description":"Call UUID"}},"required":["call_id"],"additionalProperties":false})),
        read_tool("list_verdicts", "List recent verdicts across all checks.", json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":10000},"app_id":{"type":"string"},"outcome":{"type":"string","enum":["pass","edit","block","escalate"]}},"additionalProperties":false})),
        read_tool("get_stats_overview", "Aggregate governance statistics for the last 24 hours.", json!({"type":"object","properties":{"app_id":{"type":"string"}},"additionalProperties":false})),
        read_tool("get_policy_stats", "Per-check effectiveness statistics (blocks, escalations, precision).", json!({"type":"object","properties":{"app_id":{"type":"string"},"window_hours":{"type":"integer","minimum":1,"maximum":720}},"required":["app_id"],"additionalProperties":false})),
        read_tool("get_detection_quality", "Trust score and per-axis precision from reviewer outcomes.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_feedback_effectiveness", "Feedback-loop effectiveness: resolutions, trends, agreement.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_judge_agreement", "Hybrid judge coverage and disagreement versus the heuristics.", json!({"type":"object","properties":{"days":{"type":"integer","minimum":1,"maximum":90}},"additionalProperties":false})),
        read_tool("get_latency_timeseries", "Hourly fast-path latency buckets (avg and p99).", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_cost_summary", "Spend summary (flat $1 per request): last 24h, 7-day, all-time, peak hour, per-model and per-app breakdown.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_cost_timeseries", "Hourly requests, tokens and spend for the last 24 hours.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_cost_daily", "Daily requests, tokens and spend for the last 30 days (idle days included as zero).", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_cost_anomalies", "Spend anomalies per app: hourly spike, request burst, daily spend above average, daily budget, token surge.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("list_escalations", "List human-review cases.", json!({"type":"object","properties":{"status":{"type":"string","enum":["open","in_review","resolved","all_open"]},"limit":{"type":"integer","minimum":1,"maximum":200},"offset":{"type":"integer","minimum":0}},"additionalProperties":false})),
        read_tool("get_session_thread", "Full multi-turn conversation thread for a call.", json!({"type":"object","properties":{"call_id":{"type":"string"}},"required":["call_id"],"additionalProperties":false})),
        read_tool("get_precedents", "Reviewer precedents most similar to a call or escalation.", json!({"type":"object","properties":{"call_id":{"type":"string"},"escalation_id":{"type":"string"}},"additionalProperties":false})),
        read_tool("list_audit", "Query the hash-chained audit trail.", json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":500},"app_id":{"type":"string"},"outcome":{"type":"string"},"axis":{"type":"string","enum":["performance","cost","responsibility"]},"cursor":{"type":"string"}},"additionalProperties":false})),
        read_tool("verify_audit_chain", "Verify the integrity of the audit hash chain.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_system_config", "Sanitized system configuration (no credentials).", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_health", "ControlPlane API liveness.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("get_ready", "ControlPlane API readiness and dependency status.", json!({"type":"object","properties":{},"additionalProperties":false})),
        read_tool("scan_content", "Opt-in curated adapter: run PII, toxicity, bias or hallucination scanning via the internal guardrails sidecar. Disabled unless MCP_ENABLE_INTERNAL_SCANS=true.", json!({"type":"object","properties":{"kind":{"type":"string","enum":["pii","toxicity","bias","hallucination"]},"text":{"type":"string"},"context":{"type":"string","description":"Grounding context, for hallucination checks"}},"required":["kind","text"],"additionalProperties":false})),
        ToolDefinition {
            name: "evaluate_prompt".into(),
            description: "Send a prompt through the governance proxy and return the governed response plus the fast-path correlation id and latency. Incurs an upstream model call.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "model": {"type": "string"},
                    "messages": {"type": "array","minItems":1,"maxItems":64,"items": {"type":"object","properties":{"role":{"type":"string","enum":["system","user","assistant"]},"content":{"type":"string","maxLength":20000}},"required":["role","content"],"additionalProperties":false}},
                    "app_id": {"type": "string"},
                    "session_id": {"type": "string"},
                    "max_tokens": {"type": "integer","minimum":1,"maximum":8192}
                },
                "required": ["messages"],
                "additionalProperties": false
            }),
            annotations: Some(json!({"destructiveHint": false, "idempotentHint": false, "openWorldHint": true})),
        },
        ToolDefinition {
            name: "resolve_escalation".into(),
            description: "Resolve a human-review case; records the decision as a precedent (reviewer/admin only).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "escalation_id": {"type": "string"},
                    "action": {"type": "string","enum":["confirm","override","dismiss"]},
                    "reason": {"type": "string","maxLength":2000}
                },
                "required": ["escalation_id","action"],
                "additionalProperties": false
            }),
            annotations: Some(json!({"destructiveHint": false, "idempotentHint": false})),
        },
        ToolDefinition {
            name: "update_policy".into(),
            description: "Update an application's governance policy (admin only).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "app_id": {"type": "string"},
                    "policy": {"type": "object","description":"Policy fields to update, mirroring PUT /api/v1/policies/{app_id}"}
                },
                "required": ["app_id","policy"],
                "additionalProperties": false
            }),
            annotations: Some(json!({"destructiveHint": true, "idempotentHint": true})),
        },
        ToolDefinition {
            name: "set_governance_level".into(),
            description: "Set an application's data governance level (admin only).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": {"type":"string"},"level": {"type":"string","enum":["high","medium","low"]}},
                "required": ["app_id","level"],
                "additionalProperties": false
            }),
            annotations: Some(json!({"destructiveHint": true, "idempotentHint": true})),
        },
        ToolDefinition {
            name: "apply_profile".into(),
            description: "Apply a regulatory profile to an application (admin only).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"app_id": {"type":"string"},"profile_id": {"type":"string"}},
                "required": ["app_id","profile_id"],
                "additionalProperties": false
            }),
            annotations: Some(json!({"destructiveHint": true, "idempotentHint": true})),
        },
        ToolDefinition {
            name: "update_profile".into(),
            description: "Update a regulatory profile's default thresholds (admin only).".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"profile_id": {"type":"string"},"thresholds": {"type":"object"}},
                "required": ["profile_id","thresholds"],
                "additionalProperties": false
            }),
            annotations: Some(json!({"destructiveHint": true, "idempotentHint": true})),
        },
    ]
}

fn read_tool(name: &str, description: &str, schema: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        input_schema: schema,
        annotations: Some(json!({"readOnlyHint": true})),
    }
}

/// Capability required to invoke a tool.
pub fn capability_for(name: &str) -> Option<Capability> {
    Some(match name {
        "list_apps"
        | "get_policy"
        | "list_profiles"
        | "list_requests"
        | "get_request"
        | "list_verdicts"
        | "get_stats_overview"
        | "get_policy_stats"
        | "get_detection_quality"
        | "get_feedback_effectiveness"
        | "get_judge_agreement"
        | "get_latency_timeseries"
        | "get_cost_summary"
        | "get_cost_timeseries"
        | "get_cost_daily"
        | "get_cost_anomalies"
        | "list_escalations"
        | "get_session_thread"
        | "get_precedents"
        | "list_audit"
        | "verify_audit_chain"
        | "get_system_config"
        | "get_health"
        | "get_ready"
        | "scan_content" => Capability::Read,
        "resolve_escalation" => Capability::Resolve,
        "evaluate_prompt"
        | "update_policy"
        | "set_governance_level"
        | "apply_profile"
        | "update_profile" => Capability::Write,
        _ => return None,
    })
}

// ─── Dispatch ────────────────────────────────────────────────────────────────

pub async fn call_tool(name: &str, args: &Value, ctx: &ToolContext<'_>) -> Result<Value, McpError> {
    let capability = capability_for(name).ok_or_else(|| McpError::method_not_found(name))?;
    ctx.principal.authorize(capability)?;

    let args = args.as_object().cloned().unwrap_or_default();
    let c = &ctx.correlation_id;

    match name {
        "list_apps" => ctx.client.dashboard_get("/api/v1/apps", &[], c).await,
        "get_policy" => {
            let app_id = req_uuid(&args, "app_id")?;
            ctx.principal.authorize_app(app_id)?;
            ctx.client
                .dashboard_get(&format!("/api/v1/policies/{app_id}"), &[], c)
                .await
        }
        "list_profiles" => ctx.client.dashboard_get("/api/v1/profiles", &[], c).await,
        "list_requests" => {
            let query = query_pairs(&[
                (
                    "limit",
                    Some(bounded_i64(&args, "limit", 50, 1, 500)?.to_string()),
                ),
                (
                    "offset",
                    Some(bounded_i64(&args, "offset", 0, 0, i64::MAX)?.to_string()),
                ),
                ("search", opt_str(&args, "search")),
                ("model", opt_str(&args, "model")),
                (
                    "outcome",
                    opt_enum(&args, "outcome", &["pass", "edit", "block", "escalate"])?,
                ),
                ("app_id", uuid_string(&args, "app_id")?),
            ]);
            ctx.client
                .dashboard_get("/api/v1/requests", &query, c)
                .await
        }
        "get_request" => {
            let call_id = req_uuid(&args, "call_id")?;
            ctx.client
                .dashboard_get(&format!("/api/v1/requests/{call_id}"), &[], c)
                .await
        }
        "list_verdicts" => {
            let query = query_pairs(&[
                (
                    "limit",
                    Some(bounded_i64(&args, "limit", 50, 1, 10_000)?.to_string()),
                ),
                ("app_id", uuid_string(&args, "app_id")?),
                (
                    "outcome",
                    opt_enum(&args, "outcome", &["pass", "edit", "block", "escalate"])?,
                ),
            ]);
            ctx.client
                .dashboard_get("/api/v1/verdicts/recent", &query, c)
                .await
        }
        "get_stats_overview" => {
            let query = query_pairs(&[("app_id", uuid_string(&args, "app_id")?)]);
            ctx.client
                .dashboard_get("/api/v1/stats/overview", &query, c)
                .await
        }
        "get_policy_stats" => {
            let app_id = req_uuid(&args, "app_id")?;
            ctx.principal.authorize_app(app_id)?;
            let query = query_pairs(&[
                ("app_id", Some(app_id.to_string())),
                (
                    "window_hours",
                    Some(bounded_i64(&args, "window_hours", 24, 1, 720)?.to_string()),
                ),
            ]);
            ctx.client
                .dashboard_get("/api/v1/stats/policy", &query, c)
                .await
        }
        "get_detection_quality" => {
            ctx.client
                .dashboard_get("/api/v1/metrics/detection-quality", &[], c)
                .await
        }
        "get_feedback_effectiveness" => {
            ctx.client
                .dashboard_get("/api/v1/metrics/feedback-effectiveness", &[], c)
                .await
        }
        "get_judge_agreement" => {
            let query = query_pairs(&[(
                "days",
                Some(bounded_i64(&args, "days", 1, 1, 90)?.to_string()),
            )]);
            ctx.client
                .dashboard_get("/api/v1/metrics/judge-agreement", &query, c)
                .await
        }
        "get_latency_timeseries" => {
            ctx.client
                .dashboard_get("/api/v1/metrics/latency-timeseries", &[], c)
                .await
        }
        "get_cost_summary" => {
            ctx.client
                .dashboard_get("/api/v1/cost/summary", &[], c)
                .await
        }
        "get_cost_timeseries" => {
            ctx.client
                .dashboard_get("/api/v1/cost/timeseries", &[], c)
                .await
        }
        "get_cost_daily" => {
            ctx.client
                .dashboard_get("/api/v1/cost/daily", &[], c)
                .await
        }
        "get_cost_anomalies" => {
            ctx.client
                .dashboard_get("/api/v1/cost/anomalies", &[], c)
                .await
        }
        "list_escalations" => {
            let query = query_pairs(&[
                (
                    "status",
                    opt_enum(
                        &args,
                        "status",
                        &["open", "in_review", "resolved", "all_open"],
                    )?,
                ),
                (
                    "limit",
                    Some(bounded_i64(&args, "limit", 50, 1, 200)?.to_string()),
                ),
                (
                    "offset",
                    Some(bounded_i64(&args, "offset", 0, 0, i64::MAX)?.to_string()),
                ),
            ]);
            ctx.client
                .dashboard_get("/api/v1/escalations", &query, c)
                .await
        }
        "get_session_thread" => {
            let call_id = req_uuid(&args, "call_id")?;
            ctx.client
                .dashboard_get(&format!("/api/v1/sessions/{call_id}/thread"), &[], c)
                .await
        }
        "get_precedents" => {
            let query = query_pairs(&[
                ("call_id", uuid_string(&args, "call_id")?),
                ("escalation_id", uuid_string(&args, "escalation_id")?),
            ]);
            if query.is_empty() {
                return Err(McpError::invalid_params(
                    "one of call_id or escalation_id is required",
                ));
            }
            ctx.client
                .dashboard_get("/api/v1/feedback/precedents", &query, c)
                .await
        }
        "list_audit" => {
            let query = query_pairs(&[
                (
                    "limit",
                    Some(bounded_i64(&args, "limit", 25, 1, 500)?.to_string()),
                ),
                ("app_id", uuid_string(&args, "app_id")?),
                (
                    "outcome",
                    opt_enum(&args, "outcome", &["pass", "edit", "block", "escalate"])?,
                ),
                (
                    "axis",
                    opt_enum(&args, "axis", &["performance", "cost", "responsibility"])?,
                ),
                ("cursor", opt_str(&args, "cursor")),
            ]);
            ctx.client.dashboard_get("/api/v1/audit", &query, c).await
        }
        "verify_audit_chain" => {
            ctx.client
                .dashboard_get("/api/v1/audit/verify", &[], c)
                .await
        }
        "get_system_config" => {
            let raw = ctx
                .client
                .dashboard_get("/api/v1/system/config", &[], c)
                .await?;
            Ok(sanitize_system_config(&raw))
        }
        "get_health" => ctx.client.dashboard_health(c).await,
        "get_ready" => ctx.client.dashboard_ready(c).await,
        "scan_content" => scan_content(&args, ctx).await,
        "evaluate_prompt" => evaluate_prompt(&args, ctx).await,
        "resolve_escalation" => {
            let escalation_id = req_uuid(&args, "escalation_id")?;
            let action = req_enum(&args, "action", &["confirm", "override", "dismiss"])?;
            let reason = opt_str(&args, "reason").unwrap_or_default();
            ctx.client
                .dashboard_post(
                    &format!("/api/v1/escalations/{escalation_id}/resolve"),
                    json!({ "action": action, "reason": reason }),
                    c,
                )
                .await
        }
        "update_policy" => {
            let app_id = req_uuid(&args, "app_id")?;
            ctx.principal.authorize_app(app_id)?;
            let policy = args
                .get("policy")
                .cloned()
                .filter(Value::is_object)
                .ok_or_else(|| McpError::invalid_params("policy must be an object"))?;
            ctx.client
                .dashboard_put(&format!("/api/v1/policies/{app_id}"), policy, c)
                .await
        }
        "set_governance_level" => {
            let app_id = req_uuid(&args, "app_id")?;
            ctx.principal.authorize_app(app_id)?;
            let level = req_enum(&args, "level", &["high", "medium", "low"])?;
            ctx.client
                .dashboard_put(
                    &format!("/api/v1/apps/{app_id}/governance"),
                    json!({ "level": level }),
                    c,
                )
                .await
        }
        "apply_profile" => {
            let app_id = req_uuid(&args, "app_id")?;
            ctx.principal.authorize_app(app_id)?;
            let profile_id = req_profile_id(&args, "profile_id")?;
            ctx.client
                .dashboard_post(
                    &format!("/api/v1/policies/{app_id}/profile"),
                    json!({ "profile_id": profile_id }),
                    c,
                )
                .await
        }
        "update_profile" => {
            let profile_id = req_profile_id(&args, "profile_id")?;
            let thresholds = args
                .get("thresholds")
                .cloned()
                .filter(Value::is_object)
                .ok_or_else(|| McpError::invalid_params("thresholds must be an object"))?;
            ctx.client
                .dashboard_put(&format!("/api/v1/profiles/{profile_id}"), thresholds, c)
                .await
        }
        other => Err(McpError::method_not_found(other)),
    }
    .map(|value| sanitize_response(&value))
}

// ─── Capability-specific helpers ─────────────────────────────────────────────

async fn evaluate_prompt(
    args: &Map<String, Value>,
    ctx: &ToolContext<'_>,
) -> Result<Value, McpError> {
    let messages = args
        .get("messages")
        .cloned()
        .filter(Value::is_array)
        .ok_or_else(|| McpError::invalid_params("messages must be an array"))?;

    let mut body = Map::new();
    body.insert("messages".into(), messages);
    if let Some(model) = opt_str(args, "model") {
        body.insert("model".into(), json!(model));
    }
    if let Some(app_id) = uuid_string(args, "app_id")? {
        ctx.principal.authorize_app(
            app_id
                .parse()
                .map_err(|_| McpError::invalid_params("invalid app_id"))?,
        )?;
        body.insert("app_id".into(), json!(app_id));
    }
    if let Some(session_id) = uuid_string_or_free(args, "session_id") {
        body.insert("session_id".into(), json!(session_id));
    }
    if let Some(max_tokens) = opt_i64(args, "max_tokens") {
        if !(1..=8192).contains(&max_tokens) {
            return Err(McpError::invalid_params("max_tokens out of range"));
        }
        body.insert("max_tokens".into(), json!(max_tokens));
    }

    let body = Value::Object(body);
    let serialized = serde_json::to_vec(&body).map_err(|_| McpError::internal())?;
    if serialized.len() > ctx.config.max_payload_bytes {
        return Err(McpError::payload_too_large(ctx.config.max_payload_bytes));
    }

    let response = ctx
        .client
        .proxy_messages(body, &ctx.correlation_id, None)
        .await?;

    Ok(json!({
        "governance": {
            "correlation_id": response.upstream_correlation_id.unwrap_or_else(|| ctx.correlation_id.clone()),
            "fast_path_latency_ms": response.governance_latency_ms,
            "note": "Shadow-path verdicts arrive asynchronously; query get_request with this call id to see them."
        },
        "response": sanitize_response(&response.body)
    }))
}

async fn scan_content(args: &Map<String, Value>, ctx: &ToolContext<'_>) -> Result<Value, McpError> {
    if !ctx.config.enable_internal_scans {
        return Err(McpError::forbidden("internal_scans_disabled"));
    }
    let base = ctx
        .config
        .guardrails_url
        .as_deref()
        .ok_or_else(|| McpError::invalid_params("guardrails URL is not configured"))?;

    let kind = req_enum(args, "kind", &["pii", "toxicity", "bias", "hallucination"])?;
    let text = req_str(args, "text")?;
    if text.len() > ctx.config.max_payload_bytes {
        return Err(McpError::payload_too_large(ctx.config.max_payload_bytes));
    }

    let mut body = Map::new();
    body.insert("text".into(), json!(text));
    if let Some(context) = opt_str(args, "context") {
        body.insert("prompt".into(), json!(context));
    }

    let result = ctx
        .client
        .absolute_post(
            base,
            &format!("/scan/{kind}"),
            Value::Object(body),
            &ctx.correlation_id,
        )
        .await?;
    Ok(sanitize_response(&result))
}

/// Whitelist the fields surfaced from `/api/v1/system/config`.
fn sanitize_system_config(raw: &Value) -> Value {
    const ALLOWED: &[&str] = &[
        "api_port",
        "proxy_addr",
        "upstream_provider",
        "upstream_model",
        "event_bus_mode",
        "database_connected",
        "database_engine",
        "database_name",
        "decision_judge",
        "calibration_version",
        "fusion_enabled",
    ];
    let mut out = Map::new();
    if let Some(map) = raw.as_object() {
        for key in ALLOWED {
            if let Some(value) = map.get(*key) {
                out.insert((*key).to_string(), value.clone());
            }
        }
    }
    sanitize_response(&Value::Object(out))
}

// ─── Argument validation ─────────────────────────────────────────────────────

fn query_pairs(spec: &[(&str, Option<String>)]) -> Vec<(String, String)> {
    spec.iter()
        .filter_map(|(key, value)| value.clone().map(|v| ((*key).to_string(), v)))
        .collect()
}

fn opt_str(args: &Map<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn req_str(args: &Map<String, Value>, key: &str) -> Result<String, McpError> {
    opt_str(args, key).ok_or_else(|| McpError::invalid_params(format!("{key} is required")))
}

fn req_enum(args: &Map<String, Value>, key: &str, allowed: &[&str]) -> Result<String, McpError> {
    let value = req_str(args, key)?;
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(McpError::invalid_params(format!("invalid {key}")))
    }
}

fn opt_enum(
    args: &Map<String, Value>,
    key: &str,
    allowed: &[&str],
) -> Result<Option<String>, McpError> {
    match opt_str(args, key) {
        Some(value) if allowed.contains(&value.as_str()) => Ok(Some(value)),
        Some(_) => Err(McpError::invalid_params(format!("invalid {key}"))),
        None => Ok(None),
    }
}

fn opt_i64(args: &Map<String, Value>, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

fn bounded_i64(
    args: &Map<String, Value>,
    key: &str,
    default: i64,
    min: i64,
    max: i64,
) -> Result<i64, McpError> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => {
            let n = value
                .as_i64()
                .ok_or_else(|| McpError::invalid_params(format!("{key} must be an integer")))?;
            if n < min || n > max {
                return Err(McpError::invalid_params(format!("{key} out of range")));
            }
            Ok(n)
        }
    }
}

fn opt_uuid(args: &Map<String, Value>, key: &str) -> Result<Option<Uuid>, McpError> {
    match opt_str(args, key) {
        None => Ok(None),
        Some(value) => Uuid::parse_str(&value)
            .map(Some)
            .map_err(|_| McpError::invalid_params(format!("{key} must be a UUID"))),
    }
}

fn req_uuid(args: &Map<String, Value>, key: &str) -> Result<Uuid, McpError> {
    opt_uuid(args, key)?.ok_or_else(|| McpError::invalid_params(format!("{key} is required")))
}

fn uuid_string(args: &Map<String, Value>, key: &str) -> Result<Option<String>, McpError> {
    Ok(opt_uuid(args, key)?.map(|u| u.to_string()))
}

fn uuid_string_or_free(args: &Map<String, Value>, key: &str) -> Option<String> {
    let value = opt_str(args, key)?;
    let safe = profile_id_regex().is_match(&value);
    if safe {
        Some(value)
    } else {
        None
    }
}

fn profile_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]{1,64}$").expect("valid profile id regex"))
}

fn req_profile_id(args: &Map<String, Value>, key: &str) -> Result<String, McpError> {
    let value = req_str(args, key)?;
    if profile_id_regex().is_match(&value) {
        Ok(value)
    } else {
        Err(McpError::invalid_params(format!("invalid {key}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_definition_has_a_capability() {
        for def in definitions() {
            assert!(
                capability_for(&def.name).is_some(),
                "missing capability for {}",
                def.name
            );
        }
    }

    #[test]
    fn capability_for_unknown_is_none() {
        assert!(capability_for("drop_database").is_none());
    }

    #[test]
    fn schemas_are_objects() {
        for def in definitions() {
            assert_eq!(def.input_schema["type"], "object");
        }
    }

    #[test]
    fn required_fields_are_declared() {
        let defs = definitions();
        let get_request = defs.iter().find(|d| d.name == "get_request").unwrap();
        assert_eq!(get_request.input_schema["required"][0], "call_id");
    }

    #[test]
    fn profile_ids_are_validated_against_traversal() {
        let args = json!({"profile_id": "../../etc/passwd"});
        let map = args.as_object().unwrap();
        assert!(req_profile_id(map, "profile_id").is_err());

        let ok = json!({"profile_id": "eu-financial"});
        assert_eq!(
            req_profile_id(ok.as_object().unwrap(), "profile_id").unwrap(),
            "eu-financial"
        );
    }

    #[test]
    fn uuid_validation_rejects_garbage() {
        let args = json!({"call_id": "not-a-uuid"});
        assert!(req_uuid(args.as_object().unwrap(), "call_id").is_err());
    }

    #[test]
    fn bounded_integer_rejects_out_of_range() {
        let args = json!({"limit": 100000});
        assert!(bounded_i64(args.as_object().unwrap(), "limit", 50, 1, 500).is_err());
    }

    #[test]
    fn system_config_whitelist_drops_unknown_and_secrets() {
        let raw = json!({"upstream_provider": "ollama", "api_key": "sk-leak", "secret_extra": 1});
        let out = sanitize_system_config(&raw);
        assert_eq!(out["upstream_provider"], "ollama");
        assert!(out.get("api_key").is_none());
        assert!(out.get("secret_extra").is_none());
    }
}
