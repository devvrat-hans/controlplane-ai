//! JSON-RPC 2.0 / Model Context Protocol wire types.
//!
//! Only the subset required to serve tools and resources is modelled. Unknown
//! fields are tolerated so that newer clients do not break this server, and
//! unknown methods return a clean `method_not_found`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::McpError;

pub const JSONRPC_VERSION: &str = "2.0";
pub const PROTOCOL_VERSION: &str = "2024-11-05";
pub const SERVER_NAME: &str = "controlplane-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// A parsed JSON-RPC request. Notifications have `id == None`.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    pub fn validate(&self) -> Result<(), McpError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(McpError::invalid_request("jsonrpc must be \"2.0\""));
        }
        if self.method.is_empty() {
            return Err(McpError::invalid_request("method is required"));
        }
        Ok(())
    }
}

/// Build a successful JSON-RPC response.
pub fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "result": result })
}

/// Build a JSON-RPC error response.
pub fn error(id: Value, err: &McpError) -> Value {
    let mut error = json!({ "code": err.code, "message": err.message });
    if let Some(data) = &err.data {
        error["data"] = data.clone();
    }
    json!({ "jsonrpc": JSONRPC_VERSION, "id": id, "error": error })
}

/// The `initialize` result advertised to clients.
pub fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false }
        },
        "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        "instructions": "Governed access to ControlPlane.ai. Read tools are available to all authenticated \
                         principals; escalation resolution requires the reviewer role; policy and \
                         profile changes require the admin role."
    })
}

/// A single MCP tool definition.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<Value>,
}

/// A single MCP resource definition.
#[derive(Debug, Clone, Serialize)]
pub struct ResourceDefinition {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// A resource template (`controlplane://request/{call_id}` style).
#[derive(Debug, Clone, Serialize)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
}

/// Wrap a JSON value as MCP tool content plus optional structured content.
pub fn tool_content(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string());
    let mut content = json!({
        "content": [{ "type": "text", "text": text }],
    });
    content["isError"] = json!(is_error);
    if !is_error {
        content["structuredContent"] = value;
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_has_no_id() {
        let req: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .unwrap();
        assert!(req.is_notification());
        assert!(req.validate().is_ok());
    }

    #[test]
    fn rejects_wrong_jsonrpc_version() {
        let req = JsonRpcRequest {
            jsonrpc: "1.0".into(),
            id: Some(json!(1)),
            method: "ping".into(),
            params: None,
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn success_shape() {
        let v = success(json!(7), json!({"ok": true}));
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["ok"], true);
    }

    #[test]
    fn error_shape_includes_code() {
        let v = error(json!(1), &McpError::timeout());
        assert_eq!(v["error"]["code"], crate::error::codes::TIMEOUT);
    }

    #[test]
    fn tool_content_hides_structured_on_error() {
        let v = tool_content(json!({"secret": "x"}), true);
        assert_eq!(v["isError"], true);
        assert!(v.get("structuredContent").is_none());
    }
}
