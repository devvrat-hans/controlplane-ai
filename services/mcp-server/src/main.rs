//! Binary entrypoint. Selects the transport from `MCP_TRANSPORT` and runs the
//! server. Configuration is documented in `services/mcp-server/README.md`.

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use controlplane_mcp_server::transport::{http, stdio};
use controlplane_mcp_server::{McpConfig, McpServer, Transport};

#[tokio::main]
async fn main() -> Result<()> {
    // Logs MUST go to stderr. The stdio transport owns stdout exclusively for
    // JSON-RPC framing; writing logs there would corrupt the protocol stream.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,controlplane_mcp_server=debug")),
        )
        .with_target(true)
        .with_writer(std::io::stderr)
        .init();

    let config = McpConfig::from_env()?;

    // Log the effective configuration, never the secrets.
    info!(
        transport = ?config.transport,
        dashboard = %config.dashboard_url,
        proxy = %config.proxy_url,
        allow_anonymous = config.allow_anonymous,
        rate_limit_per_min = config.rate_limit_per_min,
        request_timeout_ms = config.request_timeout.as_millis() as u64,
        internal_scans = config.enable_internal_scans,
        "ControlPlane MCP server starting"
    );
    if config.allow_anonymous {
        warn!("MCP_ALLOW_ANONYMOUS=true — unauthenticated callers get read-only viewer access");
    }

    let max_body = config.max_payload_bytes;
    let http_addr = config.http_addr.clone();
    let transport = config.transport.clone();

    let server = Arc::new(McpServer::new(config)?);
    info!(
        tools = controlplane_mcp_server::tools::definitions().len(),
        "tool surface ready"
    );

    match transport {
        Transport::Stdio => stdio::run(server).await,
        Transport::Http => {
            let listener = tokio::net::TcpListener::bind(&http_addr).await?;
            info!(addr = %http_addr, "Streamable HTTP transport listening");
            let app = http::router(server, max_body);
            axum::serve(listener, app).await?;
            Ok(())
        }
    }
}
