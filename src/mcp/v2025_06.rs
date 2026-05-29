//! MCP protocol version 2025-06-18 (Streamable HTTP)

use serde_json::Value;

pub const VERSION: &str = "2025-06-18";

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
        "instructions": "MCP Streamable HTTP (2025-06-18). POST JSON-RPC to this URL. Tools: memory_search, memory_save, memory_list, memory_forget, memory_stats."
    })
}
