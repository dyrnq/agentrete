use crate::storage::Store;
use serde_json::Value;
use std::process::Command;

use super::v2024;
use super::v2025_06;
use super::v2025_11;

pub(crate) fn tools_list() -> Value {
    serde_json::json!({"tools":[
        {"name":"memory_search","description":"Semantic search with RRF fusion (vec0 KNN + FTS5 BM25)","inputSchema":{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"number"},"type":{"type":"string"}},"required":["query"]}},
        {"name":"memory_save","description":"Save with optional dry_run preview, auto-detects project from git","inputSchema":{"type":"object","properties":{"content":{"type":"string"},"type":{"type":"string"},"tags":{"type":"string"},"source_file":{"type":"string"},"project":{"type":"string"},"dry_run":{"type":"boolean"}},"required":["content"]}},
        {"name":"memory_list","description":"List memories, optionally filtered by type","inputSchema":{"type":"object","properties":{"limit":{"type":"number"},"type":{"type":"string"},"offset":{"type":"number"}},"required":[]}},
        {"name":"memory_forget","description":"Delete","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}},
        {"name":"memory_stats","description":"Stats","inputSchema":{"type":"object","properties":{},"required":[]}},
        {"name":"memory_compact","description":"Deduplicate memories (exact or semantic) and reclaim disk space","inputSchema":{"type":"object","properties":{"mode":{"type":"string"}},"required":[]}},
        {"name":"kg_query","description":"Query knowledge graph: neighbors, shortest path, or subgraph","inputSchema":{"type":"object","properties":{"mode":{"type":"string"},"entity":{"type":"string"},"target":{"type":"string"},"predicate":{"type":"string"},"direction":{"type":"string"},"project":{"type":"string"}},"required":["mode"]}},
        {"name":"kg_scan","description":"Scan codebase with optional file watching. If watch=true, starts file watcher after scan completes.","inputSchema":{"type":"object","properties":{"path":{"type":"string"},"watch":{"type":"boolean"}},"required":["path"]}},{"name":"kg_scan_status","description":"Check if a background scan is running","inputSchema":{"type":"object","properties":{},"required":[]}}    ]})
}

/// Detect project name from git repo root, falling back to current directory.
fn detect_project() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
                std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            } else {
                None
            }
        })
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
                    let dry_run = a["dry_run"].as_bool().unwrap_or(false);
                    if dry_run {
                        let preview_id = format!("mem_{}", uuid::Uuid::new_v4());
                        return jsonrpc_ok(
                            &serde_json::Value::String(preview_id.clone()),
                            serde_json::json!({"content":[{"type":"text","text":format!("Preview: {} (not saved)", preview_id)}]}),
                        );
                    }
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
                            project: a["project"]
                                .as_str()
                                .map(|s| s.to_string())
                                .or_else(detect_project),
                            source_file: a["source_file"].as_str().map(|s| s.to_string()),
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
                    let l = a["limit"].as_u64().unwrap_or(10) as u8;
                    match store.search(q, l, a["type"].as_str()).await {
                        Ok(r) => {
                            let items: Vec<Value> = r
                                .into_iter()
                                .map(|x| {
                                    let meta = format!(
                                        "tags:{} imp:{} at:{} src:{} proj:{}",
                                        x.tags.as_ref().map(|t| t.join(",")).unwrap_or_default(),
                                        x.importance,
                                        x.created_at,
                                        x.source_file.as_deref().unwrap_or("?"),
                                        x.project.as_deref().unwrap_or("?")
                                    );
                                    let mut text = format!(
                                        "[{}] {} (score={:.2}) id={}  {}",
                                        x.memory_type.as_deref().unwrap_or("-"),
                                        x.content,
                                        x.score,
                                        x.id,
                                        meta
                                    );
                                    // Append KG context if available
                                    if store.graph.is_enabled() {
                                        if let Ok(triples) = futures::executor::block_on(
                                            store.graph.query_by_memory_id(&store.pool, &x.id),
                                        ) {
                                            for (s, p, o) in &triples {
                                                text.push_str(&format!(
                                                    "
  └─ kg: {} --[{}]--> {}",
                                                    s, p, o
                                                ));
                                            }
                                        }
                                    }
                                    serde_json::json!({"type":"text","text":text})
                                })
                                .collect();
                            jsonrpc_ok(&id, serde_json::json!({"content":items}))
                        }
                        Err(e) => jsonrpc_err(&id, -32000, &format!("Search failed: {}", e)),
                    }
                }
                "memory_list" => match store
                    .list(
                        a["limit"].as_u64().unwrap_or(20) as u8,
                        a["type"].as_str(),
                        a["offset"].as_u64().unwrap_or(0) as u32,
                    )
                    .await
                {
                    Ok(e) => {
                        let items: Vec<Value> = e
                            .into_iter()
                            .map(|m| {
                                let meta = format!(
                                    "tags:{} imp:{} at:{} src:{} proj:{}",
                                    m.tags.as_ref().map(|t| t.join(",")).unwrap_or_default(),
                                    m.importance,
                                    m.created_at,
                                    m.source_file.as_deref().unwrap_or("?"),
                                    m.project.as_deref().unwrap_or("?")
                                );
                                serde_json::json!({"type":"text","text":format!(
                                    "[{}] {} id={}  {}",
                                    m.memory_type.as_deref().unwrap_or("-"),
                                    m.content,
                                    m.id,
                                    meta
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
                "memory_compact" => {
                    let mode = a.get("mode").and_then(|v| v.as_str()).unwrap_or("exact");
                    let threshold =
                        a.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.95) as f32;
                    match store.compact(mode, threshold).await {
                        Ok((removed, remaining)) => jsonrpc_ok(
                            &id,
                            serde_json::json!({"content":[{"type":"text","text":format!("Compacted ({mode}): {} duplicates removed, {} memories remain.", removed, remaining)}]}),
                        ),
                        Err(e) => jsonrpc_err(&id, -32000, &format!("Compact failed: {}", e)),
                    }
                }
                "kg_scan" => {
                    let path = a["path"].as_str().unwrap_or(".").to_string();
                    let _do_watch = a["watch"].as_bool().unwrap_or(false);
                    if !store.graph.is_enabled() {
                        jsonrpc_err(&id, -32000, "Knowledge graph is disabled. Set [knowledge_graph] enabled = true in config.")
                    } else if store
                        .scan_running
                        .load(std::sync::atomic::Ordering::Acquire)
                    {
                        jsonrpc_err(
                            &id,
                            -32001,
                            "A scan is already in progress. Check kg_scan_status for progress.",
                        )
                    } else {
                        let store2 = store.clone();
                        let (task_id, cancel_flag, _notify) = store.tasks.register().await;
                        let task_id_clone = task_id.clone();
                        let cancel_clone = cancel_flag.clone();
                        store
                            .scan_running
                            .store(true, std::sync::atomic::Ordering::Release);
                        tokio::spawn(async move {
                            let result = match store2
                                .scan_codebase(std::path::Path::new(&path))
                                .await
                            {
                                Ok((syms, rels)) => {
                                    serde_json::json!({"ok": true, "symbols": syms, "relations": rels, "message": format!("Scanned {} symbols, {} relations", syms, rels)})
                                }
                                Err(e) => {
                                    serde_json::json!({"ok": false, "error": e.to_string()})
                                }
                            };
                            store2
                                .scan_running
                                .store(false, std::sync::atomic::Ordering::Release);
                            if cancel_clone.load(std::sync::atomic::Ordering::Acquire) == 0 {
                                store2.tasks.complete(&task_id_clone, result).await;
                            }
                        });
                        jsonrpc_ok(
                            &id,
                            serde_json::json!({"content":[{"type":"text","text":format!("Task {}: scan started. Use tasks/send to query or kg_scan_status to check.", task_id)}]}),
                        )
                    }
                }
                "kg_scan_status" => {
                    let running = store
                        .scan_running
                        .load(std::sync::atomic::Ordering::Acquire);
                    let text = if running {
                        "Scan is running...".to_string()
                    } else {
                        "No scan running. Run kg_scan to start one.".to_string()
                    };
                    jsonrpc_ok(
                        &id,
                        serde_json::json!({"content":[{"type":"text","text":text}]}),
                    )
                }
                "kg_query" => {
                    let mode = a["mode"].as_str().unwrap_or("");
                    let entity = a["entity"].as_str().unwrap_or("");
                    let predicate = a["predicate"].as_str();
                    let direction = a["direction"].as_str().unwrap_or("both");
                    if !store.graph.is_enabled() {
                        return jsonrpc_err(&id, -32000, "Knowledge graph is disabled. Set [knowledge_graph] enabled = true in config.");
                    }
                    if mode == "neighbors" || mode == "subgraph" {
                        if entity.is_empty() {
                            return jsonrpc_err(
                                &id,
                                -32002,
                                "kg_query mode='neighbors' requires 'entity'",
                            );
                        }
                        let results = store.graph.query_neighbors(entity, predicate, direction);
                        let text = if results.is_empty() {
                            format!("No relations found for '{}'", entity)
                        } else {
                            let mut s = format!(
                                "Relations for '{}':
",
                                entity
                            );
                            for (target, rel, conf) in &results {
                                s.push_str(&format!(
                                    "  {} --[{}]--> {} (conf={})
",
                                    entity, rel, target, conf
                                ));
                            }
                            s
                        };
                        jsonrpc_ok(
                            &id,
                            serde_json::json!({"content":[{"type":"text","text":text}]}),
                        )
                    } else if mode == "path" {
                        if entity.is_empty() {
                            return jsonrpc_err(
                                &id,
                                -32002,
                                "kg_query mode='path' requires 'entity'",
                            );
                        }
                        let target = a["target"].as_str().unwrap_or("");
                        if target.is_empty() {
                            return jsonrpc_err(
                                &id,
                                -32002,
                                "kg_query mode='path' requires 'target'",
                            );
                        }
                        match store.graph.query_path(entity, target) {
                            Some(path) => {
                                let text = format!("Shortest path: {}", path.join(" → "));
                                jsonrpc_ok(
                                    &id,
                                    serde_json::json!({"content":[{"type":"text","text":text}]}),
                                )
                            }
                            None => jsonrpc_err(
                                &id,
                                -32000,
                                &format!("No path found between '{}' and '{}'", entity, target),
                            ),
                        }
                    } else {
                        jsonrpc_err(
                            &id,
                            -32002,
                            &format!(
                                "kg_query: unknown mode '{}' (try 'neighbors' or 'path')",
                                mode
                            ),
                        )
                    }
                }
                "memory_stats" => match store.stats().await {
                    Ok(s) => {
                        let mut text = format!(
                            "Memories: {} ({} embeddings)\n",
                            s.memory_count, s.with_embedding
                        );
                        if let Some(ref mi) = s.model_info {
                            text.push_str(&format!("Model: {}\n", mi));
                        }
                        text.push_str(&format!(
                            "Schema: v{}\nVec0: {}\nTools: {}\n",
                            s.schema_version,
                            if s.vec0_enabled {
                                "enabled"
                            } else {
                                "disabled"
                            },
                            s.tool_count
                        ));
                        if !s.type_counts.is_empty() {
                            text.push_str("By type:\n");
                            for (t, c) in &s.type_counts {
                                text.push_str(&format!("  {}: {}\n", t, c));
                            }
                        }
                        let size_mb = s.db_size_bytes as f64 / 1_048_576.0;
                        text.push_str(&format!(
                            "Sessions: {}\nObservations: {}\nDB size: {:.1} MB\nDB path: {}",
                            s.session_count, s.observation_count, size_mb, s.db_path
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
        "tasks/send" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").unwrap_or(&serde_json::Value::Null);
            match name {
                "kg_scan" => {
                    let path = args["path"].as_str().unwrap_or(".").to_string();
                    let _do_watch = args["watch"].as_bool().unwrap_or(false);
                    if !store.graph.is_enabled() {
                        jsonrpc_err(&id, -32000, "Knowledge graph is disabled")
                    } else if store
                        .scan_running
                        .load(std::sync::atomic::Ordering::Acquire)
                    {
                        jsonrpc_err(&id, -32001, "A scan is already in progress")
                    } else {
                        let store2 = store.clone();
                        let (task_id, cancel_flag, _notify) = store.tasks.register().await;
                        let task_id_clone = task_id.clone();
                        let cancel_clone = cancel_flag.clone();
                        store
                            .scan_running
                            .store(true, std::sync::atomic::Ordering::Release);
                        tokio::spawn(async move {
                            let result = match store2
                                .scan_codebase(std::path::Path::new(&path))
                                .await
                            {
                                Ok((syms, rels)) => {
                                    serde_json::json!({"ok": true, "symbols": syms, "relations": rels})
                                }
                                Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
                            };
                            store2
                                .scan_running
                                .store(false, std::sync::atomic::Ordering::Release);
                            if cancel_clone.load(std::sync::atomic::Ordering::Acquire) == 0 {
                                store2.tasks.complete(&task_id_clone, result).await;
                            }
                        });
                        if let Some(st) = store.tasks.status(&task_id).await {
                            jsonrpc_ok(
                                &id,
                                serde_json::json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&st).unwrap_or_default()}]}),
                            )
                        } else {
                            jsonrpc_ok(
                                &id,
                                serde_json::json!({"content":[{"type":"text","text":format!("Task {} created", task_id)}]}),
                            )
                        }
                    }
                }
                _ => jsonrpc_err(&id, -32602, &format!("Unknown task: {}", name)),
            }
        }
        "tasks/cancel" => {
            let task_id = params["id"].as_str().unwrap_or("");
            if task_id.is_empty() {
                jsonrpc_err(&id, -32602, "tasks/cancel requires params.id")
            } else if store.tasks.cancel(task_id).await {
                jsonrpc_ok(
                    &id,
                    serde_json::json!({"content":[{"type":"text","text":format!("Task {} cancelled", task_id)}]}),
                )
            } else {
                jsonrpc_err(
                    &id,
                    -32000,
                    &format!("Task {} not found or already completed", task_id),
                )
            }
        }
        "tasks/status" => {
            let task_id = params["id"].as_str().unwrap_or("");
            if task_id.is_empty() {
                let all = store.tasks.all_statuses().await;
                jsonrpc_ok(
                    &id,
                    serde_json::json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&all).unwrap_or_default()}]}),
                )
            } else if let Some(st) = store.tasks.status(task_id).await {
                jsonrpc_ok(
                    &id,
                    serde_json::json!({"content":[{"type":"text","text":serde_json::to_string_pretty(&st).unwrap_or_default()}]}),
                )
            } else {
                jsonrpc_err(&id, -32000, &format!("Task {} not found", task_id))
            }
        }
        "ping" => jsonrpc_ok(&id, serde_json::json!({})),
        _ => jsonrpc_err(&Value::Null, -32601, &format!("Unknown: {}", method)),
    }
}
