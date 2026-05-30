//! MCP protocol version 2025-11-25 (Streamable HTTP, stable)
//! Supports ping and full session management as per spec.

use serde_json::Value;

pub const VERSION: &str = "2025-11-25";

pub fn handle_initialize() -> Value {
    serde_json::json!({
        "protocolVersion": VERSION,
        "serverInfo": {
            "name": env!("CARGO_PKG_NAME"),
            "title": "Agentrete Memory Server",
            "description": "Local-first persistent memory engine with BM25 + vector search for AI coding agents",
            "version": env!("CARGO_PKG_VERSION")
        },
        "capabilities": {"tools": {"listChanged": false}},
        "instructions": super::protocol::MEMORY_PROTOCOL
    })
}
