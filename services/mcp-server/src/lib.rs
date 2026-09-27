//! ControlPlane.ai Model Context Protocol server.
//!
//! This crate is a *first-party client* of the existing ControlPlane surfaces.
//! It does not re-implement governance logic: every tool and resource maps onto
//! an endpoint served by `controlplane-dashboard-api` or `controlplane-proxy`.
//! It adds the cross-cutting concerns an agent-facing protocol needs —
//! authentication, capability authorization, app-scoped isolation, rate
//! limiting, timeouts, payload bounds, correlation-id propagation, redaction and
//! sanitized errors — without changing any existing service.
//!
//! ```text
//!  MCP client ──stdio / Streamable HTTP──▶ McpServer
//!                                            ├─ auth (token → role → capability)
//!                                            ├─ rate limit (per principal)
//!                                            ├─ tools / resources (this crate)
//!                                            └─ ControlPlaneClient (reqwest)
//!                                                  ├─ dashboard-api :8080
//!                                                  └─ proxy         :8900
//! ```

pub mod auth;
pub mod client;
pub mod config;
pub mod error;
pub mod protocol;
pub mod ratelimit;
pub mod redact;
pub mod resources;
pub mod server;
pub mod tools;
pub mod transport;

pub use config::{McpConfig, Transport};
pub use error::McpError;
pub use server::McpServer;
