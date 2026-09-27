# `mcp-server` — ControlPlane.ai Model Context Protocol server

> **Status: IMPLEMENTED** (server, tools, resources, both transports, security
> controls, tests). See [§11 Status](#11-status--known-gaps) for the two
> deliberately opt-in pieces.

Exposes the governance capabilities that ControlPlane.ai already provides to any
MCP-capable agent (IDE assistants, desktop clients, orchestration frameworks) as
a small, typed, secure surface.

The server is a **first-party client** of the existing APIs. It re-implements no
governance logic: every tool maps onto an endpoint served by
`controlplane-dashboard-api` (BFF, `:8080`) or `controlplane-proxy` (`:8900`).
That keeps behaviour identical for dashboard and MCP callers, and means the MCP
server can be added or removed without ever being on the critical path.

```text
  MCP client ──stdio / Streamable HTTP──▶ McpServer
                                            ├─ auth        token → role → capability
                                            ├─ rate limit  per principal (token bucket)
                                            ├─ tools       tools.rs      (thin endpoint maps)
                                            ├─ resources   resources.rs  (read-only views)
                                            └─ ControlPlaneClient (reqwest)
                                                  ├─ dashboard-api  :8080
                                                  └─ proxy          :8900
```

## 1. Quick start

`docker compose up --build` starts this server alongside the rest of the stack
as the `mcp-server` service: HTTP transport on **`http://localhost:8090`**, with
`CONTROLPLANE_API_URL`/`CONTROLPLANE_PROXY_URL` pointed at the in-network
`gateway` and tokens taken from `MCP_AUTH_TOKENS` (default
`dev-admin:admin,dev-reviewer:reviewer,dev-viewer:viewer`).

```bash
docker compose up --build -d
docker compose ps mcp-server                 # healthcheck from GET /health
curl -s localhost:8090/health
```

To run it standalone instead:

```bash
# 1. Start the stack (gateway serves both :8080 and :8900)
docker-compose up --build -d          # or: cargo run -p controlplane-gateway

# 2. stdio transport (for local MCP clients)
MCP_TRANSPORT=stdio \
MCP_AUTH_TOKENS='dev-admin:admin,dev-reviewer:reviewer' \
MCP_TOKEN=dev-admin \          # or MCP_ROLE=viewer, but not neither
  cargo run -p controlplane-mcp-server

# 3. Streamable HTTP transport
MCP_TRANSPORT=http \
MCP_HTTP_ADDR=127.0.0.1:8090 \
MCP_AUTH_TOKENS='dev-admin:admin' \
  cargo run -p controlplane-mcp-server

curl -s localhost:8090/health
curl -s -X POST localhost:8090/mcp \
  -H 'Authorization: Bearer dev-admin' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

## 2. API → MCP mapping

Every tool below is a direct mapping. Nothing is invented; nothing internal is
exposed by default.

| MCP tool | Capability | Existing endpoint |
|---|---|---|
| `list_apps` | read | `GET /api/v1/apps` |
| `get_policy` | read | `GET /api/v1/policies/{app_id}` |
| `list_profiles` | read | `GET /api/v1/profiles` |
| `list_requests` | read | `GET /api/v1/requests` |
| `get_request` | read | `GET /api/v1/requests/{call_id}` |
| `list_verdicts` | read | `GET /api/v1/verdicts/recent` |
| `get_stats_overview` | read | `GET /api/v1/stats/overview` |
| `get_policy_stats` | read | `GET /api/v1/stats/policy` |
| `get_detection_quality` | read | `GET /api/v1/metrics/detection-quality` |
| `get_feedback_effectiveness` | read | `GET /api/v1/metrics/feedback-effectiveness` |
| `get_judge_agreement` | read | `GET /api/v1/metrics/judge-agreement` |
| `get_latency_timeseries` | read | `GET /api/v1/metrics/latency-timeseries` |
| `get_cost_summary` | read | `GET /api/v1/cost/summary` |
| `get_cost_timeseries` | read | `GET /api/v1/cost/timeseries` |
| `get_cost_anomalies` | read | `GET /api/v1/cost/anomalies` |
| `list_escalations` | read | `GET /api/v1/escalations` |
| `get_session_thread` | read | `GET /api/v1/sessions/{call_id}/thread` |
| `get_precedents` | read | `GET /api/v1/feedback/precedents` |
| `list_audit` | read | `GET /api/v1/audit` |
| `verify_audit_chain` | read | `GET /api/v1/audit/verify` |
| `get_system_config` | read | `GET /api/v1/system/config` (field whitelist) |
| `get_health` / `get_ready` | read | `GET /health`, `GET /ready` |
| `scan_content` | read | guardrails sidecar (opt-in, see §8) |
| `evaluate_prompt` | write | `POST /v1/messages` (proxy) |
| `resolve_escalation` | resolve | `POST /api/v1/escalations/{id}/resolve` |
| `update_policy` | write | `PUT /api/v1/policies/{app_id}` |
| `set_governance_level` | write | `PUT /api/v1/apps/{id}/governance` |
| `apply_profile` | write | `POST /api/v1/policies/{app_id}/profile` |
| `update_profile` | write | `PUT /api/v1/profiles/{profile_id}` |

### Resources

| URI | Content |
|---|---|
| `controlplane://apps` | applications and governance levels |
| `controlplane://profiles` | regulatory profiles |
| `controlplane://escalations/open` | open + in-review cases |
| `controlplane://audit/recent` | most recent audit records |
| `controlplane://metrics/detection-quality` | trust score / precision |
| `controlplane://metrics/judge-agreement` | judge coverage & disagreement |

Templates: `controlplane://policy/{app_id}`, `controlplane://request/{call_id}`.

### Schemas

Every tool declares a JSON Schema (`inputSchema`) with `required` fields and
`enum`/`minimum`/`maximum` constraints. Schemas are part of the contract test
suite (`tests/mcp_contract_test.rs`), so a tool cannot be added without a schema
or a capability. Example:

```jsonc
// get_policy
{ "type": "object",
  "properties": { "app_id": { "type": "string", "description": "Application UUID" } },
  "required": ["app_id"], "additionalProperties": false }

// resolve_escalation
{ "type": "object",
  "properties": {
    "escalation_id": { "type": "string" },
    "action": { "type": "string", "enum": ["confirm", "override", "dismiss"] },
    "reason": { "type": "string", "maxLength": 2000 }
  },
  "required": ["escalation_id", "action"], "additionalProperties": false }
```

## 3. Transports

| Transport | Enable | Auth source | Use |
|---|---|---|---|
| stdio | `MCP_TRANSPORT=stdio` (default) | `MCP_TOKEN` env, else `MCP_ROLE` | local clients / IDEs |
| Streamable HTTP | `MCP_TRANSPORT=http` | `Authorization: Bearer` or `X-API-Key` | hosted / remote agents |

`POST /mcp` accepts a single JSON-RPC message or a batch. Notifications return
`202 Accepted` with an empty body. **Server-sent events (`GET /mcp`) are
`TARGET`, not implemented** — this server needs request/response semantics only.

**stdout is reserved for JSON-RPC on the stdio transport.** All diagnostics go to
stderr; writing logs to stdout would corrupt the protocol stream. This is enforced
by `tests/stdio_binary_test.rs`, which runs the real binary and fails if any
non-JSON line appears on stdout.

## 4. Security model

- **Authentication** — static token registry (`MCP_AUTH_TOKENS`), constant-time
  comparison. Missing/invalid credentials are rejected; `MCP_ALLOW_ANONYMOUS=true`
  (off by default) grants a read-only `viewer` principal for local demos.
- **stdio is fail-closed** — it requires either `MCP_TOKEN` or an explicit
  `MCP_ROLE`, and refuses to start with neither. There is no implicit role.
- **Authorization** — capability based, reusing the platform's role model:

  | Capability | admin | reviewer | viewer |
  |---|:--:|:--:|:--:|
  | read | ✅ | ✅ | ✅ |
  | resolve (escalations) | ✅ | ✅ | — |
  | write (policies, profiles, evaluate) | ✅ | — | — |

  Authorization failures are JSON-RPC errors (HTTP 403), never silent no-ops.
- **Isolation** — a token can be app-scoped (`token:role:app_id|app_id`). Any
  app-targeting tool then rejects out-of-scope apps. *Note: the platform has no
  multi-tenant model beyond app scoping, so this is the strongest isolation
  available without changing existing services.*
- **Rate limiting** — per-principal token bucket (`MCP_RATE_LIMIT_PER_MIN`,
  `MCP_RATE_LIMIT_BURST`), returning `429` / JSON-RPC `-32003` with
  `retry_after_ms`.
- **Timeouts** — hard per-request timeout (`MCP_REQUEST_TIMEOUT_MS`, connect
  timeout 3s). Cancellation is safe: dropping an in-flight future aborts the
  upstream request (covered by `tests/timeout_test.rs`).
- **Payload limits** — request bodies bounded by `MCP_MAX_PAYLOAD_BYTES`
  (HTTP `413` / JSON-RPC `-32007`); upstream responses bounded by
  `MCP_MAX_RESPONSE_BYTES` before buffering.
- **Input validation** — UUIDs, enums, integer ranges and profile ids
  (`^[A-Za-z0-9_-]{1,64}$`) are validated before any URL is built, so path and
  query injection (e.g. `../../admin`) are impossible.
- **Sensitive data** — all outbound data passes through `redact.rs`: AWS keys,
  bearer tokens, `key=value` credentials, emails, SSNs and card numbers are
  replaced with `[REDACTED:*]`; `request_payload` / `response_payload` and any
  `password` / `api_key` / `token` / `secret` field are suppressed entirely;
  strings are truncated. Depth, array length and string length are bounded.
- **Error hygiene** — errors are sanitized (`error.rs`). Upstream response
  bodies are never forwarded; upstream statuses map to stable codes; internal
  error strings are never surfaced. Only the method name (caller-supplied) and a
  correlation id are echoed.
- **Correlation IDs** — one `UUIDv7` per request, sent upstream as
  `X-Request-Id` and `X-ControlPlane-Correlation-Id`, echoed in errors. The
  proxy's governed latency and correlation id are surfaced by `evaluate_prompt`.

## 5. Configuration

| Variable | Default | Purpose |
|---|---|---|
| `MCP_TRANSPORT` | `stdio` | `stdio` \| `http` |
| `CONTROLPLANE_API_URL` | `http://localhost:8080` | dashboard BFF base URL |
| `CONTROLPLANE_PROXY_URL` | `http://localhost:8900` | proxy base URL |
| `CONTROLPLANE_PROXY_API_KEY` | – | credential forwarded to the proxy |
| `MCP_AUTH_TOKENS` | – | `token:role[:app_ids]`, comma separated |
| `MCP_ALLOW_ANONYMOUS` | `false` | read-only anonymous access |
| `MCP_TOKEN` | – | stdio credential |
| `MCP_ROLE` | – (unset) | stdio role when `MCP_TOKEN` is unset; unset ⇒ stdio refuses to start |
| `MCP_HTTP_ADDR` | `127.0.0.1:8090` | HTTP bind address |
| `MCP_REQUEST_TIMEOUT_MS` | `15000` | per-request upstream timeout |
| `MCP_MAX_PAYLOAD_BYTES` | `262144` | request size limit |
| `MCP_MAX_RESPONSE_BYTES` | `2097152` | upstream response limit |
| `MCP_RATE_LIMIT_PER_MIN` | `120` | sustained rate |
| `MCP_RATE_LIMIT_BURST` | `30` | burst capacity |
| `CONTROLPLANE_GUARDRAILS_URL` | – | internal sidecar (only with scans on) |
| `MCP_ENABLE_INTERNAL_SCANS` | `false` | enable `scan_content` |

## 6. Observability

- **Logging** — `tracing` with `RUST_LOG` (`info,controlplane_mcp_server=debug`
  by default). Secrets, tokens and payloads are never logged; the effective
  configuration is logged without credentials.
- **Health** — `GET /health` (liveness) and `GET /ready` (upstream reachability;
  `503` when the dashboard API is unreachable).
- **Metrics** — `TARGET`. The platform has no metrics module; when one is added,
  the natural counters are `mcp_requests_total{method,status}`,
  `mcp_tool_calls_total{tool,capability}`, `mcp_upstream_latency_ms` and
  `mcp_rate_limited_total`. Until then, use the structured logs.

## 7. Example client usage

**Claude Desktop / IDE (`stdio`):**

```jsonc
{
  "mcpServers": {
    "controlplane": {
      "command": "cargo",
      "args": ["run", "-p", "controlplane-mcp-server"],
      "env": {
        "MCP_TRANSPORT": "stdio",
        "MCP_TOKEN": "dev-admin",
        "MCP_AUTH_TOKENS": "dev-admin:admin,dev-reviewer:reviewer",
        "CONTROLPLANE_API_URL": "http://localhost:8080"
      }
    }
  }
}
```

**HTTP (`curl`):**

```bash
curl -s -X POST localhost:8090/mcp \
  -H 'Authorization: Bearer dev-admin' \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call",
       "params":{"name":"list_escalations","arguments":{"status":"all_open","limit":5}}}'
```

## 8. The opt-in scanner adapter

PII redaction, toxicity, bias and hallucination scanning only exist **inside**
the pipeline (fast-path regex; Presidio / transformers / DeepEval in the
internal guardrails sidecar). There is no public endpoint for a one-off scan.

`scan_content` is therefore a **curated, gated adapter**: it is disabled unless
`MCP_ENABLE_INTERNAL_SCANS=true` **and** `CONTROLPLANE_GUARDRAILS_URL` is set,
it requires an authenticated principal, it bounds input and it sanitizes output.
Enable it only when the sidecar is reachable over a trusted network. When it is
off (the default), the capability is still available *indirectly* through
`evaluate_prompt` → verdicts → `get_request`.

Two capabilities are intentionally **not** exposed:

- **Direct judge scoring** (`laya-*`): the judge is opt-in and shadow-only, and
  its only public surface is verdicts and `get_judge_agreement`. Scoring
  arbitrary text would require the judge service and would put a model in the
  request path — contrary to the platform's non-negotiables.
- **Citation checking**: not implemented anywhere in the platform. `groundedness`
  is the closest existing signal and is visible through verdicts.

## 9. Local development & testing

```bash
cargo build -p controlplane-mcp-server
cargo test  -p controlplane-mcp-server          # unit + contract + security + timeout + HTTP e2e
cargo clippy -p controlplane-mcp-server --all-targets -- -D warnings
cargo fmt -p controlplane-mcp-server
```

Test layout:

| File | Covers |
|---|---|
| `src/**` unit tests | protocol, config, auth, rate limit, redaction, validation |
| `tests/mcp_contract_test.rs` | tool/resource catalogue, success paths, redaction, upstream failure |
| `tests/security_test.rs` | auth, capability, app-scope, payload limits, adapter gate |
| `tests/timeout_test.rs` | timeout mapping, cancellation safety |
| `tests/http_transport_test.rs` | end-to-end HTTP: auth headers, health/ready, batch, body limit |
| `tests/stdio_binary_test.rs` | real binary over stdio: stdout purity, fail-closed auth, parse errors |

All integration tests run against an in-process mock upstream
(`tests/common/mod.rs`) — no database, NATS or model provider required.

## 10. Health, deployment, rollout & rollback

**Deployment** — the server is a single stateless process. `docker-compose.yml`
runs it as the `mcp-server` service (image built from
`services/mcp-server/Dockerfile`, port 8090, HTTP transport, healthcheck against
`/health`); the command below is the equivalent manual deployment. In both cases
run it next to the gateway, reachable by agents only:

```bash
MCP_TRANSPORT=http MCP_HTTP_ADDR=0.0.0.0:8090 \
MCP_AUTH_TOKENS="$MCP_TOKENS" \
CONTROLPLANE_API_URL=http://gateway:8080 \
  ./controlplane-mcp
```

Bind it to a private interface and front it with the same access controls as the
dashboard API. It holds no state and writes nothing except through the existing
APIs.

**Rollout**

1. Deploy with `MCP_TRANSPORT=stdio` (or HTTP bound to loopback) and
   `MCP_AUTH_TOKENS` containing a single `viewer` token.
2. Point one agent at it; exercise `tools/list` + read tools.
3. Add `reviewer`/`admin` tokens for the workflows that need them.
4. Enable `MCP_ENABLE_INTERNAL_SCANS` only if the sidecar path is trusted.

**Rollback** — stop the process. The MCP server is not on any critical path, so
there is nothing to drain and no state to migrate; dashboard and proxy traffic
is unaffected. If a change is at fault, re-deploy the previous binary.

## 11. Status & known gaps

| Item | Status |
|---|---|
| Server, dispatch, auth, rate limiting, redaction, both transports | **IMPLEMENTED** |
| 30 tools, 6 resources, 2 templates | **IMPLEMENTED** |
| Unit / contract / security / timeout / HTTP + stdio e2e tests (90) | **IMPLEMENTED** |
| `scan_content` internal adapter | **IMPLEMENTED, opt-in (off by default)** |
| SSE streaming (`GET /mcp`), metrics counters | **TARGET** |
| Direct judge scoring, citation checking | **Not exposed — no public API exists** |

---

See `AGENTS.md` for the service contract and `docs/analysis/checks-inventory.md`
for the governance checks this surface reports on.
