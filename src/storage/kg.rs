//! Knowledge Graph integration — codebase scanning, git history import.

use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

/// Add a SPO triple to the knowledge graph.
#[allow(dead_code)]
pub(crate) async fn add_triple(
    pool: &SqlitePool,
    graph: &crate::knowledge_graph::KnowledgeGraph,
    subject: &str,
    predicate: &str,
    object: &str,
    confidence: f32,
    source_memory_id: Option<String>,
    project: Option<String>,
) -> Result<String> {
    let id = format!("triple_{}", uuid::Uuid::new_v4());
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,source_memory_id,project,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)")
        .bind(&id).bind(subject).bind(predicate).bind(object).bind(confidence).bind(&source_memory_id).bind(&project).bind(&now)
        .execute(pool).await?;
    graph.add_triple_local(subject, predicate, object, confidence, source_memory_id);
    Ok(id)
}

/// Scan a codebase directory with tree-sitter and import results into KG.
pub(crate) async fn scan_codebase(
    pool: &SqlitePool,
    graph: &crate::knowledge_graph::KnowledgeGraph,
    root: &std::path::Path,
) -> Result<(usize, usize)> {
    let (symbols, relations) = crate::knowledge_graph::scanner::scan_directory(root)?;
    let now = chrono::Utc::now().to_rfc3339();
    let project = detect_project_for_scan(root);

    for sym in &symbols {
        let id = format!("node_{}", uuid::Uuid::new_v4());
        let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'label',?3,1.0,?4,?5)")
            .bind(&id).bind(&sym.name).bind(&sym.kind).bind(&project).bind(&now).execute(pool).await;
    }
    for rel in &relations {
        let id = format!("rel_{}", uuid::Uuid::new_v4());
        let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,?3,?4,1.0,?5,?6)")
            .bind(&id).bind(&rel.source).bind(&rel.relation).bind(&rel.target).bind(&project).bind(&now).execute(pool).await;
    }
    for sym in &symbols {
        graph.add_triple_local(&sym.name, "label", &sym.kind, 1.0, None);
    }
    for rel in &relations {
        graph.add_triple_local(&rel.source, &rel.relation, &rel.target, 1.0, None);
    }
    if std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok()
    {
        if let Some(ref git_root) = detect_project_git_root(root) {
            if let Err(e) = scan_git_history(pool, git_root, &project).await {
                log::warn!("kg_scan: git history scan failed: {e}");
            }
        }
    }
    log::info!(
        "kg_scan: {} symbols, {} relations from {:?}",
        symbols.len(),
        relations.len(),
        root
    );
    Ok((symbols.len(), relations.len()))
}

/// Stop file watcher.
#[allow(dead_code)]
pub(crate) fn stop_watch(
    watch_handle: &Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
) {
    if let Ok(mut h) = watch_handle.lock() {
        if let Some(handle) = h.take() {
            handle.abort();
            log::info!("kg_watch: stopped");
        }
    }
}

/// Scan git history and write commit/file relationships.
pub(crate) async fn scan_git_history(
    pool: &SqlitePool,
    git_root: &std::path::Path,
    project: &Option<String>,
) -> Result<()> {
    use std::process::Command;
    let now = chrono::Utc::now().to_rfc3339();
    let output = Command::new("git")
        .args([
            "log",
            "--name-only",
            "--pretty=format:COMMIT%x00%H%x00%s%x00%an%x00%ai",
            "-100",
            "--diff-filter=AM",
        ])
        .current_dir(git_root)
        .output()?;
    if !output.status.success() {
        return Ok(());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let entries: Vec<&str> = stdout.split("COMMIT").filter(|s| !s.is_empty()).collect();

    for entry in entries {
        let lines: Vec<&str> = entry.lines().collect();
        if let Some(header) = lines.first() {
            let parts: Vec<&str> = header.split('\0').collect();
            let commit_hash = parts.get(1).copied().unwrap_or("");
            let message = parts.get(2).copied().unwrap_or("");
            let author = parts.get(3).copied().unwrap_or("");
            if !commit_hash.is_empty() {
                let commit_id = format!("commit:{}", &commit_hash[..commit_hash.len().min(8)]);
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'hash',?3,1.0,?4,?5)")
                    .bind(format!("ch_{}",uuid::Uuid::new_v4())).bind(&commit_id).bind(commit_hash).bind(project).bind(&now).execute(pool).await;
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'message',?3,1.0,?4,?5)")
                    .bind(format!("cm_{}",uuid::Uuid::new_v4())).bind(&commit_id).bind(message).bind(project).bind(&now).execute(pool).await;
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'author',?3,1.0,?4,?5)")
                    .bind(format!("ca_{}",uuid::Uuid::new_v4())).bind(&commit_id).bind(author).bind(project).bind(&now).execute(pool).await;
            }
            for file in lines.iter().skip(1) {
                let file = file.trim();
                if file.is_empty() {
                    continue;
                }
                let file_id = format!("file:{}", file);
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'label','file',1.0,?3,?4)")
                    .bind(format!("fl_{}",uuid::Uuid::new_v4())).bind(&file_id).bind(project).bind(&now).execute(pool).await;
                if !commit_hash.is_empty() {
                    let commit_id = format!("commit:{}", &commit_hash[..commit_hash.len().min(8)]);
                    let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'changed',?3,1.0,?4,?5)")
                        .bind(format!("fc_{}",uuid::Uuid::new_v4())).bind(&commit_id).bind(&file_id).bind(project).bind(&now).execute(pool).await;
                }
            }
        }
    }
    Ok(())
}

// ─── Project detection helpers ───────────────────────────────────────────────

fn detect_project_git_root(root: &std::path::Path) -> Option<std::path::PathBuf> {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if p.is_empty() {
                    None
                } else {
                    Some(std::path::PathBuf::from(p))
                }
            } else {
                None
            }
        })
}

fn detect_project_for_scan(root: &std::path::Path) -> Option<String> {
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string());
            if name.is_some() {
                return name;
            }
        }
    }
    root.file_name().map(|n| n.to_string_lossy().to_string())
}
