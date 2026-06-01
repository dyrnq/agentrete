//! Knowledge graph tool implementations — scan, query, status.

use crate::storage::Store;
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

pub async fn kg_scan(store: &Store, path: &str, force: bool) -> Result<Value> {
    if !store.graph.is_enabled() {
        anyhow::bail!(
            "Knowledge graph is disabled. Set [knowledge_graph] enabled = true in config."
        );
    }
    if store.scan_running.load(Ordering::Acquire) {
        anyhow::bail!("A scan is already in progress.");
    }
    let (task_id, cancel_flag, _notify) = store.tasks.register().await;
    let task_id_clone = task_id.clone();
    let cancel_clone = cancel_flag.clone();
    let store2 = store.clone();
    let owned_path = path.to_string();
    store.scan_running.store(true, Ordering::Release);
    tokio::spawn(async move {
        if force {
            let project =
                crate::storage::detect_project_for_scan(std::path::Path::new(&owned_path));
            let branch = crate::storage::detect_git_branch(std::path::Path::new(&owned_path));
            let _ = store2.clear_kg(&project, &branch).await;
        }
        let result = match store2
            .scan_codebase(std::path::Path::new(&owned_path))
            .await
        {
            Ok((syms, rels)) => {
                json!({"ok": true, "symbols": syms, "relations": rels, "message": format!("Scanned {} symbols, {} relations", syms, rels)})
            }
            Err(e) => json!({"ok": false, "error": e.to_string()}),
        };
        store2.scan_running.store(false, Ordering::Release);
        if cancel_clone.load(Ordering::Acquire) == 0 {
            store2.tasks.complete(&task_id_clone, result).await;
        }
    });
    Ok(json!({"content": [{"type": "text", "text": format!("Task {}: scan started.", task_id)}]}))
}

pub async fn kg_scan_status(store: &Store) -> Result<Value> {
    let running = store.scan_running.load(Ordering::Acquire);
    let text = if running {
        "Scan is running..."
    } else {
        "No scan running. Run kg_scan to start one."
    };
    Ok(json!({"content": [{"type": "text", "text": text}]}))
}

pub async fn kg_query(
    store: &Store,
    mode: &str,
    entity: Option<&str>,
    target: Option<&str>,
    predicate: Option<&str>,
    direction: Option<&str>,
    project: Option<&str>,
) -> Result<Value> {
    if !store.graph.is_enabled() {
        anyhow::bail!(
            "Knowledge graph is disabled. Set [knowledge_graph] enabled = true in config."
        );
    }
    let dir = direction.unwrap_or("both");
    let ent = entity.unwrap_or("");
    let text = match mode {
        "neighbors" | "subgraph" => {
            if ent.is_empty() {
                anyhow::bail!("kg_query mode='neighbors' requires 'entity'");
            }
            let results = store
                .kg_query_neighbors(ent, predicate, dir, project)
                .await?;
            if results.is_empty() {
                format!("No relations found for '{ent}'")
            } else {
                let mut s = format!("Relations for '{ent}':\n");
                for (subj, rel, obj, conf) in &results {
                    s.push_str(&format!("  {subj} --[{rel}]--> {obj} (conf={conf})\n"));
                }
                s
            }
        }
        "path" => {
            if ent.is_empty() {
                anyhow::bail!("kg_query mode='path' requires 'entity'");
            }
            let tgt = target.unwrap_or("");
            if tgt.is_empty() {
                anyhow::bail!("kg_query mode='path' requires 'target'");
            }
            match store.kg_query_path(ent, tgt, project).await? {
                Some(path) => format!("Shortest path: {}", path.join(" -> ")),
                None => anyhow::bail!("No path found between '{ent}' and '{tgt}'"),
            }
        }
        other => anyhow::bail!("kg_query: unknown mode '{other}' (try 'neighbors' or 'path')"),
    };
    Ok(json!({"content": [{"type": "text", "text": text}]}))
}
