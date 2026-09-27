//! stdio transport: newline-delimited JSON-RPC over stdin/stdout.
//!
//! Intended for local MCP clients (IDE agents, desktop clients). The credential
//! is read from the environment (`MCP_TOKEN`) or falls back to `MCP_ROLE`, so
//! nothing sensitive is ever passed as a command-line argument (where it would
//! be visible in a process listing).

use std::sync::Arc;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::server::McpServer;

pub async fn run(server: Arc<McpServer>) -> anyhow::Result<()> {
    let principal = server.stdio_principal()?;

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parsed: Result<Value, _> = serde_json::from_str(line);
        let response = match parsed {
            Ok(value) => server.handle_message_with(value, principal.clone()).await,
            Err(_) => Some(crate::protocol::error(
                Value::Null,
                &crate::error::McpError::parse_error(),
            )),
        };

        if let Some(response) = response {
            let serialized = serde_json::to_string(&response)?;
            stdout.write_all(serialized.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}
