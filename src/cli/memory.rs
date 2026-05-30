use anyhow::Result;
use crate::storage::Store;
use crate::types;

pub(crate) async fn cmd_save(
    store: &Store, content: String, r#type: Option<String>,
    tags: Option<String>, files: Option<String>, project: Option<String>,
) -> Result<()> {
    let tags_vec = tags.map(|t| t.split(',').map(|s| s.trim().to_string()).collect());
    let files_vec = files.map(|f| f.split(',').map(|s| s.trim().to_string()).collect());
    let id = store.save(types::NewMemory {
        content, memory_type: r#type, tags: tags_vec, files: files_vec,
        project, source_file: None,
    }).await?;
    println!("Saved memory: {}", id);
    Ok(())
}

pub(crate) async fn cmd_search(store: &Store, query: String, limit: u8, r#type: Option<String>) -> Result<()> {
    let results = store.search(&query, limit, r#type.as_deref()).await?;
    if results.is_empty() {
        println!("No memories found.");
    } else {
        for m in &results {
            println!("[{}] {} (score={:.2})  id={}", m.memory_type.as_deref().unwrap_or("-"), m.content, m.score, m.id);
        }
    }
    Ok(())
}

pub(crate) async fn cmd_list(store: &Store, limit: u8) -> Result<()> {
    let entries = store.list(limit, None, 0).await?;
    if entries.is_empty() {
        println!("No memories.");
    } else {
        for m in &entries {
            println!("[{}] {}  id={}", m.memory_type.as_deref().unwrap_or("-"), m.content, m.id);
        }
    }
    Ok(())
}

pub(crate) async fn cmd_stats(store: &Store) -> Result<()> {
    let stats = store.stats().await?;
    println!("Memories: {}", stats.memory_count);
    Ok(())
}

pub(crate) async fn cmd_forget(store: &Store, id: String) -> Result<()> {
    store.forget(&id).await?;
    println!("Deleted.");
    Ok(())
}

pub(crate) async fn cmd_wipe(store: &Store, force: bool) -> Result<()> {
    if force { store.wipe().await?; println!("All memories deleted."); }
    else { println!("Use --force to confirm."); }
    Ok(())
}

