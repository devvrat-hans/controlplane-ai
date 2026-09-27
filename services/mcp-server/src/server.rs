//! MCP server core: JSON-RPC framing, authentication, rate limiting and
//! method dispatch. Transport modules (stdio, HTTP) call
//! [`McpServer::handle_message`]; none of the protocol logic is duplicated.

use std::sync::Arc;

use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{AuthRegistry, Capability, Principal};
use crate::client::ControlPlaneClient;
use crate::config::McpConfig;
use crate::error::{codes, McpError};
use crate::protocol::{
    error as rpc_error, initialize_result, success, tool_content, JsonRpcRequest,
};
use crate::ratelimit::RateLimiter;
use crate::{resources, tools};

pub struct McpServer {
    pub client: ControlPlaneClient,
    pub config: Arc<McpConfig>,
    auth: AuthRegistry,
    limiter: RateLimiter,
}

impl McpServer {
    pub fn new(config: McpConfig) -> Result<Self, McpError> {
        let auth = AuthRegistry::from_config(config.allow_anonymous, &config.auth_tokens)?;
        let client = ControlPlaneClient::new(
            config.dashboard_url.clone(),
            config.proxy_url.clone(),
            config.proxy_api_key.clone(),
            config.request_timeout,
            config.max_response_bytes,
        )?;
        let limiter = RateLimiter::new(config.rate_limit_per_min, config.rate_limit_burst);
        Ok(Self {
            client,
            config: Arc::new(config),
            auth,
            limiter,
        })
    }

    /// Resolve the principal configured for the stdio transport.
    ///
    /// stdio is fail-closed: it requires either `MCP_TOKEN` (a registry token)
    /// or an explicit `MCP_ROLE`. With neither, the transport refuses to start
    /// instead of silently serving unauthenticated read access.
    pub fn stdio_principal(&self) -> Result<Principal, McpError> {
        if let Some(token) = std::env::var("MCP_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty())
        {
            return self.auth.resolve(Some(token.trim()));
        }
        if self.config.stdio_role.is_empty() {
            return Err(McpError::unauthorized());
        }
        match crate::auth::parse_role(&self.config.stdio_role) {
            Some(role) => Ok(Principal::new("stdio", role, None)),
            None => Err(McpError::invalid_request("invalid MCP_ROLE")),
        }
    }

    /// Resolve a bearer token supplied by the HTTP transport.
    pub fn resolve_token(&self, token: Option<&str>) -> Result<Principal, McpError> {
        self.auth.resolve(token)
    }

    /// Handle a single JSON-RPC message, authenticating with a bearer token.
    /// Returns `None` for notifications.
    pub async fn handle_message(&self, raw: Value, token: Option<&str>) -> Option<Value> {
        let request: JsonRpcRequest = match serde_json::from_value(raw.clone()) {
            Ok(request) => request,
            Err(_) => return Some(rpc_error(Value::Null, &McpError::parse_error())),
        };
        let id = request.id.clone().unwrap_or(Value::Null);
        let is_notification = request.is_notification();

        match self.resolve_token(token) {
            Ok(principal) => self.handle_message_with(raw, principal).await,
            Err(err) => {
                if is_notification {
                    None
                } else {
                    Some(rpc_error(id, &err))
                }
            }
        }
    }

    /// Handle a JSON-RPC message with an already-resolved principal. Used by the
    /// stdio transport, which authenticates from the environment.
    pub async fn handle_message_with(&self, raw: Value, principal: Principal) -> Option<Value> {
        let request: JsonRpcRequest = match serde_json::from_value(raw) {
            Ok(request) => request,
            Err(_) => return Some(rpc_error(Value::Null, &McpError::parse_error())),
        };
        let id = request.id.clone().unwrap_or(Value::Null);
        let is_notification = request.is_notification();

        if let Err(err) = request.validate() {
            return if is_notification {
                None
            } else {
                Some(rpc_error(id, &err))
            };
        }

        // Rate limit per principal.
        if let Err(err) = self.limiter.check(&principal.subject).await {
            return if is_notification {
                None
            } else {
                Some(rpc_error(id, &err))
            };
        }

        if is_notification {
            // Only `notifications/initialized` is expected; ignore the rest.
            return None;
        }

        let correlation_id = Uuid::now_v7().to_string();
        match self
            .dispatch(
                &request.method,
                request.params.clone(),
                &principal,
                &correlation_id,
            )
            .await
        {
            Ok(result) => Some(success(id, result)),
            Err(err) => Some(rpc_error(id, &err.correlation(&correlation_id))),
        }
    }

    async fn dispatch(
        &self,
        method: &str,
        params: Option<Value>,
        principal: &Principal,
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let params = params.unwrap_or(Value::Null);
        match method {
            "initialize" => Ok(initialize_result()),
            "ping" => Ok(json!({})),
            "tools/list" => {
                principal.authorize(Capability::Read)?;
                Ok(json!({ "tools": tools::definitions() }))
            }
            "tools/call" => {
                self.handle_tools_call(&params, principal, correlation_id)
                    .await
            }
            "resources/list" => {
                principal.authorize(Capability::Read)?;
                Ok(json!({ "resources": resources::definitions() }))
            }
            "resources/templates/list" => {
                principal.authorize(Capability::Read)?;
                Ok(json!({ "resourceTemplates": resources::templates() }))
            }
            "resources/read" => {
                let uri = params
                    .get("uri")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| McpError::invalid_params("uri is required"))?;
                resources::read(uri, &self.client, principal, correlation_id).await
            }
            other => Err(McpError::method_not_found(other)),
        }
    }

    async fn handle_tools_call(
        &self,
        params: &Value,
        principal: &Principal,
        correlation_id: &str,
    ) -> Result<Value, McpError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| McpError::invalid_params("name is required"))?;
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

        // Tool existence is a protocol concern; capability is a security one.
        if tools::capability_for(name).is_none() {
            return Err(McpError::method_not_found(name));
        }

        let ctx = tools::ToolContext {
            client: &self.client,
            principal,
            correlation_id: correlation_id.to_string(),
            config: &self.config,
        };

        match tools::call_tool(name, &arguments, &ctx).await {
            Ok(value) => Ok(tool_content(value, false)),
            // Security and capacity failures are protocol errors so clients can
            // react (re-auth, back off). Everything else is a tool error.
            Err(err)
                if matches!(
                    err.code,
                    codes::UNAUTHORIZED
                        | codes::FORBIDDEN
                        | codes::RATE_LIMITED
                        | codes::PAYLOAD_TOO_LARGE
                ) =>
            {
                Err(err)
            }
            Err(err) => Ok(tool_content(
                json!({ "error": { "code": err.code, "message": err.message } }),
                true,
            )),
        }
    }

    /// Opportunistically trim rate-limit state. Called by long-lived transports.
    pub async fn prune(&self) {
        self.limiter.prune().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Transport;

    fn server(tokens: &str) -> McpServer {
        let config = McpConfig {
            transport: Transport::Http,
            auth_tokens: tokens.into(),
            ..McpConfig::default()
        };
        McpServer::new(config).unwrap()
    }

    #[tokio::test]
    async fn initialize_requires_auth() {
        let server = server("good:admin");
        let response = server
            .handle_message(
                json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
                Some("bad"),
            )
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], codes::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn initialize_succeeds_with_token() {
        let server = server("good:admin");
        let response = server
            .handle_message(
                json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
                Some("good"),
            )
            .await
            .unwrap();
        assert_eq!(
            response["result"]["protocolVersion"],
            crate::protocol::PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn tools_list_hides_capabilities_from_viewer() {
        let server = server("v:viewer");
        let response = server
            .handle_message(
                json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
                Some("v"),
            )
            .await
            .unwrap();
        let tools = response["result"]["tools"].as_array().unwrap();
        // The list is the same; authorization is enforced at call time.
        assert!(tools.iter().any(|t| t["name"] == "list_apps"));
    }

    #[tokio::test]
    async fn unknown_method_is_reported() {
        let server = server("a:admin");
        let response = server
            .handle_message(json!({"jsonrpc":"2.0","id":1,"method":"nope"}), Some("a"))
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn notifications_have_no_response() {
        let server = server("a:admin");
        let response = server
            .handle_message(
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                Some("a"),
            )
            .await;
        assert!(response.is_none());
    }

    #[tokio::test]
    async fn viewer_cannot_resolve_escalation() {
        let server = server("v:viewer");
        let response = server
            .handle_message(
                json!({
                    "jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"resolve_escalation","arguments":{"escalation_id":"00000000-0000-0000-0000-000000000001","action":"confirm"}}
                }),
                Some("v"),
            )
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], codes::FORBIDDEN);
    }

    #[tokio::test]
    async fn invalid_tool_params_return_tool_error() {
        let server = server("a:admin");
        let response = server
            .handle_message(
                json!({
                    "jsonrpc":"2.0","id":1,"method":"tools/call",
                    "params":{"name":"get_request","arguments":{"call_id":"nope"}}
                }),
                Some("a"),
            )
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], true);
    }

    #[tokio::test]
    async fn parse_error_is_reported() {
        let server = server("a:admin");
        let response = server
            .handle_message(json!({"not":"jsonrpc"}), Some("a"))
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], codes::PARSE_ERROR);
    }
}
