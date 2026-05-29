use crate::storage::Store;
use serde_json::Value;

use super::v2024;
use super::v2025_06;
use super::v2025_11;

pub(crate) fn tools_list() -> Value {
    serde_json::json!({"tools":[
        {"name":"memory_search","description":"Search","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"number"}},"required":["query"]}},
        {"name":"memory_save","description":"Save","inputSchema":{"type":"object","properties":{"content":{"type":"string"},"type":{"type":"string"},"tags":{"type":"string"}},"required":["content"]}},
        {"name":"memory_list","description":"List","inputSchema":{"type":"object","properties":{"limit":{"type":"number"}},"required":[]}},
        {"name":"memory_forget","description":"Delete","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
        {"name":"memory_stats","description":"Stats","inputSchema":{"type":"object","properties":{},"required":[]}}
    ]})
}

pub(crate) fn jsonrpc_ok(id: &Value, r: Value) -> Value {
    serde_json::json!({"jsonrpc":"2.0","id":id,"result":r})
}

pub(crate) fn jsonrpc_err(id: &Value, c: i64, m: &str) -> Value {
    serde_json::json!({"jsonrpc":"2.0","id":id,"error":{"code":c,"message":m}})
}

pub(crate) async fn handle_rpc(store: &Store, method: &str, params: &Value) -> Value {
    let id = params.get("id").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            // params is request["params"], so protocolVersion is directly under it
            let requested_version = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let supported = &["2025-11-25", "2025-06-18", "2024-11-05"];
            if supported.contains(&requested_version) {
                match requested_version {
                    "2024-11-05" => jsonrpc_ok(&id, v2024::handle_initialize()),
                    "2025-06-18" => jsonrpc_ok(&id, v2025_06::handle_initialize()),
                    _ => jsonrpc_ok(&id, v2025_11::handle_initialize()),
                }
            } else {
                jsonrpc_err(
                    &id,
                    -32602,
                    &format!(
                        "Unsupported protocol version '{}'. Supported: {:?}",
                        requested_version, supported
                    ),
                )
            }
        }
        "tools/list" => jsonrpc_ok(&id, tools_list()),
        "tools/call" => {
            let n = params["name"].as_str().unwrap_or("");
            let a = params.get("arguments").unwrap_or(&Value::Null);
            match n {
                "memory_save" => {
                    let c = a["content"].as_str().unwrap_or("");
                    let mt = a["type"].as_str().map(|s| s.to_string());
                    let tags = a["tags"]
                        .as_str()
                        .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
                    match store
                        .save(crate::types::NewMemory {
                            content: c.to_string(),
                            memory_type: mt,
                            tags,
                            files: None,
                            project: None,
                        })
                        .await
                    {
                        Ok(id) => jsonrpc_ok(
                            &serde_json::Value::String(id.clone()),
                            serde_json::json!({"content":[{"type":"text","text":format!("Saved: {}",id)}]}),
                        ),
                        Err(e) => jsonrpc_err(&id, -32000, &format!("Save failed: {}", e)),
                    }
                }
                "memory_search" => {
                    let q = a["query"].as_str().unwrap_or("");
                    let l = a["limit"].as_u64().unwrap_or(5) as u8;
                    match store.search(q, l, a["type"].as_str()).await {
                        Ok(r) => {
                            let items: Vec<Value> = r
                                .into_iter()
                                .map(|x| {
                                    serde_json::json!({"type":"text","text":format!(
                                        "[{}] {} (score={:.2}) id={}",
                                        x.memory_type.as_deref().unwrap_or("-"),
                                        x.content,
                                        x.score,
                                        x.id
                                    )})
                                })
                                .collect();
                            jsonrpc_ok(&id, serde_json::json!({"content":items}))
                        }
                        Err(e) => jsonrpc_err(&id, -32000, &format!("Search failed: {}", e)),
                    }
                }
                "memory_list" => match store.list(a["limit"].as_u64().unwrap_or(10) as u8).await {
                    Ok(e) => {
                        let items: Vec<Value> = e
                            .into_iter()
                            .map(|m| {
                                serde_json::json!({"type":"text","text":format!(
                                    "[{}] {} id={}",
                                    m.memory_type.as_deref().unwrap_or("-"),
                                    m.content,
                                    m.id
                                )})
                            })
                            .collect();
                        jsonrpc_ok(&id, serde_json::json!({"content":items}))
                    }
                    Err(e) => jsonrpc_err(&id, -32000, &format!("List failed: {}", e)),
                },
                "memory_forget" => match store.forget(a["id"].as_str().unwrap_or("")).await {
                    Ok(()) => jsonrpc_ok(
                        &id,
                        serde_json::json!({"content":[{"type":"text","text":format!("Deleted: {}",a["id"].as_str().unwrap_or(""))}]}),
                    ),
                    Err(e) => jsonrpc_err(&id, -32000, &format!("Forget failed: {}", e)),
                },
                "memory_stats" => match store.stats().await {
                    Ok(s) => {
                        let mut text = format!(
                            "Memories: {} ({} embeddings)\n",
                            s.memory_count, s.with_embedding
                        );
                        if let Some(ref mi) = s.model_info {
                            text.push_str(&format!("Model: {}\n", mi));
                        }
                        if !s.type_counts.is_empty() {
                            text.push_str("By type:\n");
                            for (t, c) in &s.type_counts {
                                text.push_str(&format!("  {}: {}\n", t, c));
                            }
                        }
                        text.push_str(&format!(
                            "Sessions: {}\nObservations: {}\nDB: {}",
                            s.session_count, s.observation_count, s.db_path
                        ));
                        jsonrpc_ok(
                            &id,
                            serde_json::json!({"content":[{"type":"text","text":text}]}),
                        )
                    }
                    Err(e) => jsonrpc_err(&id, -32000, &format!("Stats failed: {}", e)),
                },
                _ => jsonrpc_err(&id, -32601, &format!("Unknown tool: {}", n)),
            }
        }
        "ping" => jsonrpc_ok(&id, serde_json::json!({})),
        _ => jsonrpc_err(&Value::Null, -32601, &format!("Unknown: {}", method)),
    }
}
