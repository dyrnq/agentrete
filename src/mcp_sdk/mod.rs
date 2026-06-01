//! MCP v2 — rmcp SDK-based implementation.
//! Activated via config: `[mcp] protocol_version = "2025"` (default: "2024" = legacy).
//! Fallback to hand-rolled v1 (`src/mcp/`) if SDK fails or config says so.

pub mod server;
