//! Memory tool implementations — search, save, list, forget, compact, stats.

use crate::storage::Store;
use crate::types::{DbStats, NewMemory};
use anyhow::Result;
use serde_json::{json, Value};

pub async fn memory_search(
    store: &Store,
    query: &str,
    limit: u8,
    type_filter: Option<&str>,
) -> Result<Value> {
    let results = store.search(query, limit, type_filter).await?;
    let items: Vec<Value> = results
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
            json!({
                "type": "text",
                "text": format!(
                    "[{}] {} (score={:.2}) id={}  {}",
                    x.memory_type.as_deref().unwrap_or("-"),
                    x.content, x.score, x.id, meta
                )
            })
        })
        .collect();
    Ok(json!({"content": items}))
}

pub async fn memory_save(
    store: &Store,
    content: &str,
    memory_type: Option<&str>,
    tags: Option<&str>,
    project: Option<&str>,
    source_file: Option<&str>,
    dry_run: bool,
) -> Result<Value> {
    if dry_run {
        let preview_id = format!("mem_{}", uuid::Uuid::new_v4());
        return Ok(
            json!({"content": [{"type": "text", "text": format!("Preview: {} (not saved)", preview_id)}]}),
        );
    }
    let parsed_tags = tags.map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
    let resolved_project = project
        .map(|s| s.to_string())
        .or_else(super::detect_project);
    let id = store
        .save(NewMemory {
            content: content.to_string(),
            memory_type: memory_type.map(|s| s.to_string()),
            tags: parsed_tags,
            files: None,
            project: resolved_project,
            source_file: source_file.map(|s| s.to_string()),
        })
        .await?;
    Ok(json!({"content": [{"type": "text", "text": format!("Saved: {}", id)}]}))
}

pub async fn memory_list(
    store: &Store,
    limit: u8,
    type_filter: Option<&str>,
    offset: u32,
) -> Result<Value> {
    let memories = store.list(limit, type_filter, offset).await?;
    let items: Vec<Value> = memories
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
            json!({
                "type": "text",
                "text": format!(
                    "[{}] {} id={}  {}",
                    m.memory_type.as_deref().unwrap_or("-"),
                    m.content, m.id, meta
                )
            })
        })
        .collect();
    Ok(json!({"content": items}))
}

pub async fn memory_forget(store: &Store, id: &str) -> Result<Value> {
    store.forget(id).await?;
    Ok(json!({"content": [{"type": "text", "text": format!("Deleted: {}", id)}]}))
}

pub async fn memory_stats(store: &Store) -> Result<Value> {
    let s: DbStats = store.stats().await?;
    let text = format!(
        "Memories: {} ({})\nModel: {}\nVec0: {}\nSessions: {}\nObs: {}\nDB: {:.1}MB",
        s.memory_count,
        s.with_embedding,
        s.model_info.as_deref().unwrap_or("none"),
        if s.vec0_enabled {
            "enabled"
        } else {
            "disabled"
        },
        s.session_count,
        s.observation_count,
        s.db_size_bytes as f64 / (1024.0 * 1024.0)
    );
    Ok(json!({"content": [{"type": "text", "text": text}]}))
}

pub async fn memory_compact(store: &Store, mode: &str) -> Result<Value> {
    let (removed, remaining) = store.compact(mode, 0.95).await?;
    let text = format!(
        "Compacted ({mode}): {} duplicates removed, {} memories remain.",
        removed, remaining
    );
    Ok(json!({"content": [{"type": "text", "text": text}]}))
}
