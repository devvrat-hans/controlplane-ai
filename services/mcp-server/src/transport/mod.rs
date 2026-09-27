//! Transport layer. Both transports share the same [`crate::server::McpServer`]
//! dispatch; they differ only in how a message and its credential arrive.

pub mod http;
pub mod stdio;
