//! MCP server — sqlx + axum (full async, Send+Sync).

mod v2024;
mod v2025_06;
mod v2025_11;

use crate::storage::Store;
use serde_json::Value;
use std::sync::Arc;

fn tools_list() -> Value { serde_json::json!({"tools":[
    {"name":"memory_search","description":"Search","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"number"}},"required":["query"]}},
    {"name":"memory_save","description":"Save","inputSchema":{"type":"object","properties":{"content":{"type":"string"},"type":{"type":"string"},"tags":{"type":"string"}},"required":["content"]}},
    {"name":"memory_list","description":"List","inputSchema":{"type":"object","properties":{"limit":{"type":"number"}},"required":[]}},
    {"name":"memory_forget","description":"Delete","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
    {"name":"memory_stats","description":"Stats","inputSchema":{"type":"object","properties":{},"required":[]}}
]}) }

fn jsonrpc_ok(id: &Value, r: Value) -> Value { serde_json::json!({"jsonrpc":"2.0","id":id,"result":r}) }
fn jsonrpc_err(id: &Value, c: i64, m: &str) -> Value { serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":c,"message":m}}) }

async fn handle_rpc(store: &Store, method: &str, params: &Value) -> Value {
    let id = params.get("id").cloned().unwrap_or(Value::Null);
    
    match method {
        "initialize" => jsonrpc_ok(&id, serde_json::json!({"protocolVersion":"2025-11-25","capabilities":{"tools":{"listChanged":false}},"serverInfo":{"name":"agentrete","title":"Agentrete Memory Server","version":env!("CARGO_PKG_VERSION"),"description":"Local-first persistent memory engine"},"instructions":"MCP Streamable HTTP (2025-11-25)."})),
        "tools/list" => jsonrpc_ok(&id, tools_list()),
        "tools/call" => {
            let n = params["name"].as_str().unwrap_or(""); let a = params.get("arguments").unwrap_or(&Value::Null);
            match n {
                "memory_save" => {
                    let c = a["content"].as_str().unwrap_or("");
                    let mt = a["type"].as_str().map(|s| s.to_string());
                    let tags = a["tags"].as_str().map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
                    match store.save(crate::types::NewMemory{content:c.to_string(),memory_type:mt,tags,files:None,project:None}).await {
                        Ok(id) => jsonrpc_ok(&serde_json::Value::String(id.clone()), serde_json::json!({"content":[{"type":"text","text":format!("Saved: {}",id)}]})),
                        Err(e) => jsonrpc_err(&id, -32000, &format!("Save failed: {}",e)),
                    }
                }
                "memory_search" => {
                    let q = a["query"].as_str().unwrap_or(""); let l = a["limit"].as_u64().unwrap_or(5) as u8;
                    match store.search(q, l, a["type"].as_str()).await {
                        Ok(r) => {
                            let items: Vec<Value> = r.into_iter().map(|x| serde_json::json!({"type":"text","text":format!("[{}] {} (score={:.2}) id={}",x.memory_type.as_deref().unwrap_or("-"),x.content,x.score,x.id)})).collect();
                            jsonrpc_ok(&id, serde_json::json!({"content":items}))
                        }
                        Err(e) => jsonrpc_err(&id, -32000, &format!("Search failed: {}",e)),
                    }
                }
                "memory_list" => match store.list(a["limit"].as_u64().unwrap_or(10) as u8).await {
                    Ok(e) => {
                        let items: Vec<Value> = e.into_iter().map(|m| serde_json::json!({"type":"text","text":format!("[{}] {} id={}",m.memory_type.as_deref().unwrap_or("-"),m.content,m.id)})).collect();
                        jsonrpc_ok(&id, serde_json::json!({"content":items}))
                    }
                    Err(e) => jsonrpc_err(&id, -32000, &format!("List failed: {}",e)),
                },
                "memory_forget" => match store.forget(a["id"].as_str().unwrap_or("")).await {
                    Ok(()) => jsonrpc_ok(&id, serde_json::json!({"content":[{"type":"text","text":format!("Deleted: {}",a["id"].as_str().unwrap_or(""))}]})),
                    Err(e) => jsonrpc_err(&id, -32000, &format!("Forget failed: {}",e)),
                },
                "memory_stats" => match store.stats().await {
                    Ok(s) => jsonrpc_ok(&id, serde_json::json!({"content":[{"type":"text","text":format!("Memories: {}\nDB: {}",s.memory_count,s.db_path)}]})),
                    Err(e) => jsonrpc_err(&id, -32000, &format!("Stats failed: {}",e)),
                },
                _ => jsonrpc_err(&id, -32601, &format!("Unknown tool: {}", n)),
            }
        }
        "ping" => jsonrpc_ok(&id, serde_json::json!({})),
        _ => jsonrpc_err(&Value::Null, -32601, &format!("Unknown: {}",method)),
    }
}

// stdio
pub async fn run_stdio(store: Store) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    eprintln!("agentrete MCP server (stdio)");
    let store = Arc::new(store);
    let stdin = BufReader::new(tokio::io::stdin()); let mut stdout = tokio::io::stdout();
    let mut lines = stdin.lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() { continue; }
        let rq: Value = match serde_json::from_str(&line) {
            Ok(v) => v, Err(e) => {
                let err = jsonrpc_err(&Value::Null, -32700, &format!("Parse error: {}", e));
                let _ = stdout.write_all(serde_json::to_string(&err).unwrap_or_default().as_bytes()).await;
                let _ = stdout.write_all(b"\n").await; let _ = stdout.flush().await; continue;
            }
        };
        let result = handle_rpc(&store, rq["method"].as_str().unwrap_or(""), &rq["params"]).await;
        let _ = stdout.write_all(serde_json::to_string(&result).unwrap_or_default().as_bytes()).await;
        let _ = stdout.write_all(b"\n").await; let _ = stdout.flush().await;
    }
    Ok(())
}

// HTTP (axum)
use axum::{extract::State, routing::post, Router};

pub async fn run_http(store: Store, config: &crate::config::Config) -> anyhow::Result<()> {
    let port = config.port;
    eprintln!("agentrete MCP server on http://127.0.0.1:{}", port);
    let state = Arc::new(store);
    let app = Router::new()
        .route("/", post(http_mcp_handler).get(http_health))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn http_mcp_handler(State(store): State<Arc<Store>>, body: String) -> impl axum::response::IntoResponse {
    let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let result = handle_rpc(&store, request["method"].as_str().unwrap_or(""), &request["params"]).await;
    axum::Json(result)
}

async fn http_health() -> axum::Json<Value> {
    axum::Json(serde_json::json!({"service":"agentrete","status":"ok","version":env!("CARGO_PKG_VERSION")}))
}
