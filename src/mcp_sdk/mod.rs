//! MCP SDK backend — rmcp-based implementation.
//! Activated via config: `[mcp] backend = "sdk"` (default: "native").
//! Falls back to native hand-rolled (`src/mcp/`) if SDK fails.

pub mod server;
pub mod tools;
