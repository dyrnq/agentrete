//! MCP server — sqlx + axum (full async, Send+Sync).

mod v2024;
mod v2025_06;
mod v2025_11;

mod handlers;
mod transport_http;
mod transport_stdio;

pub use transport_http::run_http;
pub use transport_stdio::run_stdio;
