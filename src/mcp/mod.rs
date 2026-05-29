//! MCP server with version-negotiated protocol handlers.
//!
//! Supports stdio and Streamable HTTP transports.
//! Protocol versions: 2024-11-05, 2025-06-18, 2025-11-25.

mod v2024;
mod v2025_06;
mod v2025_11;

use std::sync::Arc;

use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::storage::Store;
use crate::types::NewMemory;

// --- JSON-RPC helpers ---

fn jsonrpc_error(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn jsonrpc_success(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

// --- Version routing ---

const SUPPORTED_VERSIONS: &[&str] = &["2025-06-18", "2025-11-25", "2024-11-05"];

fn dispatch_initialize(client_version: &str) -> Value {
    match client_version {
        v2024::VERSION => v2024::handle_initialize(),
        v2025_06::VERSION => v2025_06::handle_initialize(),
        v2025_11::VERSION => v2025_11::handle_initialize(),
        _ => unreachable!("version should be validated before dispatch"),
    }
}

// --- Tools (version-independent) ---

fn handle_tools_list() -> Value {
    json!({"tools": [
        {"name": "memory_search", "description": "Search past memories",
         "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "number"}}, "required": ["query"]}},
        {"name": "memory_save", "description": "Save to long-term memory",
         "inputSchema": {"type": "object", "properties": {"content": {"type": "string"}, "type": {"type": "string"}, "tags": {"type": "string"}}, "required": ["content"]}},
        {"name": "memory_list", "description": "List recent memories",
         "inputSchema": {"type": "object", "properties": {"limit": {"type": "number"}}, "required": []}},
        {"name": "memory_forget", "description": "Delete a memory by ID",
         "inputSchema": {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}},
        {"name": "memory_stats", "description": "Show memory statistics",
         "inputSchema": {"type": "object", "properties": {}, "required": []}},
    ]})
}

async fn handle_tool_call_impl(store: &Store, name: &str, args: &Value) -> Option<Value> {
    match name {
        "memory_search" => {
            let query = args["query"].as_str().unwrap_or("").to_string();
            let limit = args["limit"].as_u64().unwrap_or(5) as u8;
            store.search(&query, limit, None).await.ok().map(|results| {
                json!({"content": results.iter().map(|m| {
                    json!({"type": "text", "text": format!("[{}] {} (score={:.2}) id={}", m.memory_type.as_deref().unwrap_or("-"), m.content, m.score, m.id)})
                }).collect::<Vec<_>>()})
            })
        }
        "memory_save" => {
            let new_mem = NewMemory {
                content: args["content"].as_str().unwrap_or("").to_string(),
                memory_type: args["type"].as_str().map(|s| s.to_string()),
                tags: args["tags"]
                    .as_str()
                    .map(|s| s.split(',').map(|t| t.trim().to_string()).collect()),
                files: None,
                project: None,
            };
            store
                .save(new_mem)
                .await
                .ok()
                .map(|id| json!({"content": [{"type": "text", "text": format!("Saved: {}", id)}]}))
        }
        "memory_list" => {
            let limit = args["limit"].as_u64().unwrap_or(10) as u8;
            store.list(limit).await.ok().map(|entries| {
                json!({"content": entries.iter().map(|m| {
                    json!({"type": "text", "text": format!("[{}] {} id={}", m.memory_type.as_deref().unwrap_or("-"), m.content, m.id)})
                }).collect::<Vec<_>>()})
            })
        }
        "memory_stats" => store.stats().await.ok().map(|st| {
            json!({"content": [{"type": "text", "text": format!(
                "Memories: {}\nDB: {}", st.memory_count, st.db_path
            )}]})
        }),
        "memory_forget" => {
            let id = args["id"].as_str().unwrap_or("").to_string();
            store
                .forget(&id)
                .await
                .ok()
                .map(|_| json!({"content": [{"type": "text", "text": format!("Deleted: {}", id)}]}))
        }
        _ => None,
    }
}

// --- RPC dispatch ---

enum RpcResult {
    #[allow(dead_code)]
    Response(Value),
    NotificationAccepted,
}

async fn handle_rpc(store: &Store, request: &Value) -> RpcResult {
    let method = request["method"].as_str().unwrap_or("");
    let has_id = request.get("id").is_some_and(|v| !v.is_null());

    if !has_id {
        return RpcResult::NotificationAccepted;
    }

    let id = request.get("id").unwrap();

    let result = match method {
        "initialize" => {
            let client_version = request["params"]["protocolVersion"].as_str().unwrap_or("");
            if SUPPORTED_VERSIONS.contains(&client_version) {
                Some(jsonrpc_success(id, dispatch_initialize(client_version)))
            } else {
                Some(jsonrpc_error(
                    id,
                    -32602,
                    &format!(
                        "Unsupported protocol version '{}'. Supported: {:?}",
                        client_version, SUPPORTED_VERSIONS
                    ),
                ))
            }
        }
        "ping" => Some(jsonrpc_success(id, json!({}))),
        "tools/list" => Some(jsonrpc_success(id, handle_tools_list())),
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let args = &request["params"]["arguments"];
            handle_tool_call_impl(store, name, args)
                .await
                .map(|r| jsonrpc_success(id, r))
        }
        _ => Some(jsonrpc_error(
            id,
            -32601,
            &format!("Unknown method: {}", method),
        )),
    };

    match result {
        Some(r) => RpcResult::Response(r),
        None => RpcResult::NotificationAccepted,
    }
}

// --- Stdio Transport ---

pub async fn run_stdio(store: Store) -> anyhow::Result<()> {
    let store = Arc::new(Mutex::new(store));
    eprintln!("agentrete MCP server (stdio)");

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line.trim()) {
            Ok(request) => {
                let s = store.lock().await;
                match handle_rpc(&s, &request).await {
                    RpcResult::Response(response) => {
                        let _ = stdout
                            .write_all(
                                serde_json::to_string(&response)
                                    .unwrap_or_default()
                                    .as_bytes(),
                            )
                            .await;
                        let _ = stdout.write_all(b"\n").await;
                        let _ = stdout.flush().await;
                    }
                    RpcResult::NotificationAccepted => {}
                }
            }
            Err(e) => {
                let err = jsonrpc_error(&json!(null), -32700, &format!("Parse error: {}", e));
                let _ = stdout
                    .write_all(serde_json::to_string(&err).unwrap_or_default().as_bytes())
                    .await;
                let _ = stdout.write_all(b"\n").await;
                let _ = stdout.flush().await;
            }
        }
    }
    Ok(())
}

// --- HTTP Transport (actix-web) ---

use actix_web::{web, App, HttpResponse, HttpServer};

pub async fn run_http(store: Store, config: &crate::config::Config) -> anyhow::Result<()> {
    let store = web::Data::new(Mutex::new(store));
    let port = config.port;
    eprintln!("agentrete MCP server on http://127.0.0.1:{}", port);

    HttpServer::new(move || {
        App::new()
            .app_data(store.clone())
            .route("/", web::post().to(http_mcp_handler))
            .route("/", web::get().to(http_health))
    })
    .bind(format!("127.0.0.1:{}", port))?
    .run()
    .await?;

    Ok(())
}

async fn http_mcp_handler(store: web::Data<Mutex<Store>>, body: web::Json<Value>) -> HttpResponse {
    let req: Value = body.into_inner();
    let s = store.lock().await;
    match handle_rpc(&s, &req).await {
        RpcResult::Response(response) => HttpResponse::Ok()
            .insert_header(("Content-Type", "application/json"))
            .json(response),
        RpcResult::NotificationAccepted => HttpResponse::Accepted()
            .insert_header(("Content-Type", "application/json"))
            .finish(),
    }
}

async fn http_health() -> HttpResponse {
    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "service": env!("CARGO_PKG_NAME"),
        "version": env!("CARGO_PKG_VERSION")
    }))
}
