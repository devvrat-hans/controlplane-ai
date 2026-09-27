//! Read-only MCP resources.
//!
//! Resources are a curated, cacheable view over the same read endpoints the
//! tools use. They never mutate state and never expose raw payloads.

use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::Principal;
use crate::client::{sanitize_response, ControlPlaneClient};
use crate::error::McpError;
use crate::protocol::{ResourceDefinition, ResourceTemplate};

pub const MIME_JSON: &str = "application/json";

pub fn definitions() -> Vec<ResourceDefinition> {
    vec![
        resource(
            "controlplane://apps",
            "Applications",
            "Registered applications and governance levels",
        ),
        resource(
            "controlplane://profiles",
            "Regulatory profiles",
            "Available regulatory governance profiles",
        ),
        resource(
            "controlplane://escalations/open",
            "Open escalations",
            "Open and in-review escalation cases",
        ),
        resource(
            "controlplane://audit/recent",
            "Recent audit",
            "Most recent hash-chained audit records",
        ),
        resource(
            "controlplane://metrics/detection-quality",
            "Detection quality",
            "Trust score and per-axis precision",
        ),
        resource(
            "controlplane://metrics/judge-agreement",
            "Judge agreement",
            "Hybrid judge coverage and disagreement",
        ),
    ]
}

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate {
            uri_template: "controlplane://policy/{app_id}".into(),
            name: "Application policy".into(),
            description: "Governance policy for one application".into(),
            mime_type: MIME_JSON.into(),
        },
        ResourceTemplate {
            uri_template: "controlplane://request/{call_id}".into(),
            name: "Request detail".into(),
            description: "Verdicts, audit records and escalation for one intercepted call".into(),
            mime_type: MIME_JSON.into(),
        },
    ]
}

fn resource(uri: &str, name: &str, description: &str) -> ResourceDefinition {
    ResourceDefinition {
        uri: uri.into(),
        name: name.into(),
        description: description.into(),
        mime_type: MIME_JSON.into(),
    }
}

/// Read a resource, returning an MCP `contents` payload.
pub async fn read(
    uri: &str,
    client: &ControlPlaneClient,
    principal: &Principal,
    correlation_id: &str,
) -> Result<Value, McpError> {
    principal.authorize(crate::auth::Capability::Read)?;

    let value = match uri {
        "controlplane://apps" => {
            client
                .dashboard_get("/api/v1/apps", &[], correlation_id)
                .await?
        }
        "controlplane://profiles" => {
            client
                .dashboard_get("/api/v1/profiles", &[], correlation_id)
                .await?
        }
        "controlplane://escalations/open" => {
            client
                .dashboard_get(
                    "/api/v1/escalations",
                    &[
                        ("status".into(), "all_open".into()),
                        ("limit".into(), "50".into()),
                    ],
                    correlation_id,
                )
                .await?
        }
        "controlplane://audit/recent" => {
            client
                .dashboard_get(
                    "/api/v1/audit",
                    &[("limit".into(), "25".into())],
                    correlation_id,
                )
                .await?
        }
        "controlplane://metrics/detection-quality" => {
            client
                .dashboard_get("/api/v1/metrics/detection-quality", &[], correlation_id)
                .await?
        }
        "controlplane://metrics/judge-agreement" => {
            client
                .dashboard_get("/api/v1/metrics/judge-agreement", &[], correlation_id)
                .await?
        }
        other => {
            if let Some(app_id) = other.strip_prefix("controlplane://policy/") {
                let app_id = Uuid::parse_str(app_id)
                    .map_err(|_| McpError::invalid_params("invalid app_id in resource uri"))?;
                principal.authorize_app(app_id)?;
                client
                    .dashboard_get(&format!("/api/v1/policies/{app_id}"), &[], correlation_id)
                    .await?
            } else if let Some(call_id) = other.strip_prefix("controlplane://request/") {
                let call_id = Uuid::parse_str(call_id)
                    .map_err(|_| McpError::invalid_params("invalid call_id in resource uri"))?;
                client
                    .dashboard_get(&format!("/api/v1/requests/{call_id}"), &[], correlation_id)
                    .await?
            } else {
                return Err(McpError::not_found("Resource"));
            }
        }
    };

    let value = sanitize_response(&value);
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into());
    Ok(json!({
        "contents": [{ "uri": uri, "mimeType": MIME_JSON, "text": text }]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_resource_uri_is_distinct() {
        let defs = definitions();
        let mut uris: Vec<_> = defs.iter().map(|d| d.uri.clone()).collect();
        uris.sort();
        uris.dedup();
        assert_eq!(uris.len(), defs.len());
    }

    #[test]
    fn templates_are_reported() {
        let templates = templates();
        assert!(templates
            .iter()
            .any(|t| t.uri_template.contains("{app_id}")));
        assert!(templates
            .iter()
            .any(|t| t.uri_template.contains("{call_id}")));
    }

    #[tokio::test]
    async fn unknown_resource_is_not_found() {
        let client = ControlPlaneClient::new(
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            None,
            std::time::Duration::from_millis(10),
            1024,
        )
        .unwrap();
        let principal = Principal::new("t", controlplane_common::types::UserRole::Admin, None);
        let err = read("controlplane://nope", &client, &principal, "c")
            .await
            .unwrap_err();
        assert_eq!(err.code, crate::error::codes::NOT_FOUND);
    }

    #[tokio::test]
    async fn invalid_uuid_resource_is_rejected() {
        let client = ControlPlaneClient::new(
            "http://127.0.0.1:1",
            "http://127.0.0.1:1",
            None,
            std::time::Duration::from_millis(10),
            1024,
        )
        .unwrap();
        let principal = Principal::new("t", controlplane_common::types::UserRole::Admin, None);
        let err = read("controlplane://policy/not-a-uuid", &client, &principal, "c")
            .await
            .unwrap_err();
        assert_eq!(err.code, crate::error::codes::INVALID_PARAMS);
    }
}
