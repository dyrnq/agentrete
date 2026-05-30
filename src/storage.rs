//! SQLite storage via sqlx (async, Send+Sync, connection pool).

use anyhow::Result;
use chrono::Utc;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::embed::embeddings::Embedder;
use crate::knowledge_graph::KnowledgeGraph;
use crate::types::{DbStats, Memory, NewMemory, SearchResult};

// Embedded sqlite-vec extension for the current platform.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
// Embedded sqlite-vec extension for the current platform.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const VEC_EXT_BYTES: &[u8] = include_bytes!("../ext/vec0-linux-x86_64.so");
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
const VEC_EXT_BYTES: &[u8] = &[];

// ─── Tunable constants (overridable via config) ──────────────────────────────
const MAX_LIMIT: u8 = 100;
const RECALL_MULTIPLIER: u8 = 3;

#[derive(Clone)]
pub struct Store {
    pub(crate) pool: SqlitePool,
    path: PathBuf,
    embedder: Option<Arc<Embedder>>,
    pub(crate) graph: KnowledgeGraph,
    pub(crate) scan_running: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) watch_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub(crate) scan_result: Arc<std::sync::Mutex<Option<String>>>,
    vec_enabled: bool,
    vec_dims: usize,
    rrf_k: f64,
    half_life_days: f64,
    _default_limit: u8,
    _list_limit: u8,
}

/// Reciprocal Rank Fusion: merge vec0 KNN and FTS5 BM25 ranked lists.
/// RRF score = sum(1 / (K + rank)) across lists, with K=60.
/// Returns top-k results sorted by RRF score descending.
fn rrf_merge(
    vec_results: Vec<SearchResult>,
    fts_results: Vec<SearchResult>,
    k: usize,
    rrf_k: f64,
) -> Vec<SearchResult> {
    use std::collections::HashMap;

    let mut scores: HashMap<&str, f64> = HashMap::new();
    let mut data: HashMap<&str, &SearchResult> = HashMap::new();

    for (rank, r) in vec_results.iter().enumerate() {
        *scores.entry(r.id.as_str()).or_default() += 1.0 / (rrf_k + rank as f64 + 1.0);
        data.entry(r.id.as_str()).or_insert(r);
    }
    for (rank, r) in fts_results.iter().enumerate() {
        *scores.entry(r.id.as_str()).or_default() += 1.0 / (rrf_k + rank as f64 + 1.0);
        data.entry(r.id.as_str()).or_insert(r);
    }

    let mut merged: Vec<(&str, f64)> = scores.into_iter().collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    merged
        .into_iter()
        .take(k)
        .filter_map(|(id, score)| {
            data.get(id).map(|r| SearchResult {
                id: r.id.clone(),
                memory_type: r.memory_type.clone(),
                content: r.content.clone(),
                tags: r.tags.clone(),
                files: r.files.clone(),
                project: r.project.clone(),
                source_file: r.source_file.clone(),
                importance: r.importance,
                score,
                created_at: r.created_at.clone(),
                embedding: r.embedding.clone(),
            })
        })
        .collect()
}

impl Store {
    pub async fn open(cfg: &crate::config::Config, embedder: Option<Embedder>) -> Result<Self> {
        let dims = match cfg.embedding.backend {
            crate::config::EmbeddingBackend::None => {
                log::info!("backend=none, dims=0, vec disabled");
                0
            }
            crate::config::EmbeddingBackend::Remote => {
                cfg.embedding.remote.dims.unwrap_or(768) as usize
            }
            _ => cfg.embedding.model2vec.dims as usize,
        };
        let path = cfg.db_dir().join("memory.db");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true)
            .foreign_keys(true);

        // Load sqlite-vec extension via connection options (pre-pool)
        let mut vec_enabled = false;
        if !VEC_EXT_BYTES.is_empty() {
            let tmp_dir = std::env::temp_dir().join("agentrete");
            std::fs::create_dir_all(&tmp_dir).ok();
            let ext_name = match (std::env::consts::OS, std::env::consts::ARCH) {
                ("linux", "x86_64") => "vec0-linux-x86_64.so",
                ("linux", "aarch64") => "vec0-linux-aarch64.so",
                ("macos", "x86_64") => "vec0-macos-x86_64.dylib",
                ("macos", "aarch64") => "vec0-macos-aarch64.dylib",
                ("windows", "x86_64") => "vec0-windows-x86_64.dll",
                _ => "none",
            };
            let ext_path = tmp_dir.join(ext_name);
            match std::fs::write(&ext_path, VEC_EXT_BYTES) {
                Ok(()) => {
                    log::info!("sqlite-vec extension extracted to {}", ext_path.display());
                    unsafe {
                        opts = opts.extension_with_entrypoint(
                            ext_path.to_string_lossy().into_owned(),
                            "sqlite3_vec_init",
                        );
                    }
                    vec_enabled = true;
                }
                Err(e) => {
                    log::warn!("failed to write sqlite-vec extension: {e}");
                }
            }
        }

        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&pool)
            .await?;

        // vec_enabled already set during extension loading above

        let vec_enabled = vec_enabled && dims > 0;
        let store = Self {
            pool,
            path,
            embedder: embedder.map(Arc::new),
            graph: KnowledgeGraph::disabled(),
            scan_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            watch_handle: Arc::new(std::sync::Mutex::new(None)),
            scan_result: Arc::new(std::sync::Mutex::new(None)),
            vec_enabled,
            vec_dims: dims,
            rrf_k: cfg.search.rrf_k,
            half_life_days: cfg.search.half_life_days,
            _default_limit: cfg.search.default_limit,
            _list_limit: cfg.search.list_limit,
        };
        if vec_enabled {
            store.init_vec().await?;
        }
        store.initialize().await?;
        // Build KG if enabled (after initialize so table exists)
        let store = if cfg.knowledge_graph.enabled {
            let kg = KnowledgeGraph::build(&store.pool).await?;
            Self { graph: kg, ..store }
        } else {
            store
        };
        Ok(store)
    }

    async fn initialize(&self) -> Result<()> {
        sqlx::query("CREATE TABLE IF NOT EXISTS _schema_version (version INTEGER PRIMARY KEY, migrated_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS memories (id TEXT PRIMARY KEY, type TEXT, content TEXT NOT NULL, tags TEXT, files TEXT, project TEXT, source_file TEXT, importance INTEGER DEFAULT 3, embedding BLOB, embedding_model TEXT, embedding_dims INTEGER, created_at TEXT, updated_at TEXT, deleted_at TEXT)").execute(&self.pool).await?;
        let _ = sqlx::query("ALTER TABLE memories ADD COLUMN source_file TEXT")
            .execute(&self.pool)
            .await;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_embed_null ON memories(embedding) WHERE embedding IS NULL").execute(&self.pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at)")
            .execute(&self.pool)
            .await?;
        sqlx::query("CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content, content_rowid='rowid', tokenize='unicode61')").execute(&self.pool).await?;
        // FTS auto-sync: INSERT trigger
        sqlx::query("CREATE TRIGGER IF NOT EXISTS memories_fts_ai AFTER INSERT ON memories WHEN new.deleted_at IS NULL BEGIN INSERT INTO memories_fts(rowid, content) VALUES (new.rowid, new.content); END;").execute(&self.pool).await?;
        // FTS auto-sync: soft-delete removes from FTS
        sqlx::query("CREATE TRIGGER IF NOT EXISTS memories_fts_au AFTER UPDATE OF deleted_at ON memories WHEN new.deleted_at IS NOT NULL AND old.deleted_at IS NULL BEGIN INSERT INTO memories_fts(memories_fts, rowid, content) VALUES ('delete', old.rowid, old.content); END;").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, data TEXT, metadata TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS observations (id TEXT PRIMARY KEY, content TEXT, tool_name TEXT, session_id TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        // Knowledge Graph triples (optional, only created if config enables it)
        let _ = sqlx::query("CREATE TABLE IF NOT EXISTS kg_triples (id TEXT PRIMARY KEY, subject TEXT NOT NULL, predicate TEXT NOT NULL, object TEXT NOT NULL, confidence REAL DEFAULT 1.0, source_memory_id TEXT, project TEXT, created_at TEXT NOT NULL)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_subject ON kg_triples(subject)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_object ON kg_triples(object)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_predicate ON kg_triples(predicate)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_memory ON kg_triples(source_memory_id)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_project ON kg_triples(project)").execute(&self.pool).await;
        let _ = sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_kg_triples_spo ON kg_triples(subject, predicate, object, project)").execute(&self.pool).await;
        Ok(())
    }

    /// Add a SPO triple to the knowledge graph.
    pub async fn add_triple(
        &self,
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
            .execute(&self.pool).await?;
        self.graph.add_triple_local(subject, predicate, object, confidence, source_memory_id);
        Ok(id)
    }

    /// Scan a codebase directory with tree-sitter and import results into KG.
    pub async fn scan_codebase(&self, root: &std::path::Path) -> Result<(usize, usize)> {
        let (symbols, relations) = crate::knowledge_graph::scanner::scan_directory(root)?;
        let now = chrono::Utc::now().to_rfc3339();
        let project = detect_project_for_scan(root);

        for sym in &symbols {
            let id = format!("node_{}", uuid::Uuid::new_v4());
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'label',?3,1.0,?4,?5)"
            )
            .bind(&id).bind(&sym.name).bind(&sym.kind).bind(&project).bind(&now)
            .execute(&self.pool).await;
        }

        for rel in &relations {
            let id = format!("rel_{}", uuid::Uuid::new_v4());
            let _ = sqlx::query(
                "INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,?3,?4,1.0,?5,?6)"
            )
            .bind(&id).bind(&rel.source).bind(&rel.relation).bind(&rel.target).bind(&project).bind(&now)
            .execute(&self.pool).await;
        }

        // Sync petgraph
        // Sync all symbols and relations
        for sym in &symbols {
            self.graph.add_triple_local(&sym.name, "label", &sym.kind, 1.0, None);
        }
        for rel in &relations {
            self.graph.add_triple_local(&rel.source, &rel.relation, &rel.target, 1.0, None);
        }

        if let Ok(mut r) = self.scan_result.lock() {
            *r = Some(format!("kg_scan: {} symbols, {} relations", symbols.len(), relations.len()));
        }
        if std::process::Command::new("git").arg("--version").output().is_ok() {
            if let Some(ref git_root) = detect_project_git_root(root) {
                if let Err(e) = self.scan_git_history(git_root, &project).await {
                    log::warn!("kg_scan: git history scan failed: {e}");
                }
            }
        }
        log::info!("kg_scan: {} symbols, {} relations from {:?}", symbols.len(), relations.len(), root);
        Ok((symbols.len(), relations.len()))
    }

    /// Start file watcher: auto-scan on code changes.
    pub fn start_watch(&self, root: &std::path::Path) {
        use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
        use std::sync::mpsc;
        let root = root.to_path_buf();
        let scan_running = self.scan_running.clone();
        let store_clone = self.clone();
        let (tx, rx) = mpsc::channel::<Result<notify::Event, notify::Error>>();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => { log::warn!("kg_watch: failed to create watcher: {e}"); return; }
        };
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            log::warn!("kg_watch: failed to watch {root:?}: {e}"); return;
        }
        let handle = tokio::spawn(async move {
            log::info!("kg_watch: watching {root:?} for changes");
            loop {
                match rx.recv() {
                    Ok(Ok(event)) => {
                        let relevant = event.paths.iter().any(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e, "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "java" | "go" | "rb" | "php" | "swift" | "kt" | "c" | "cpp" | "h" | "hpp" | "cs" | "scala" | "sh" | "bash" | "zsh")));
                        if !relevant { continue; }
                        if matches!(event.kind, EventKind::Modify(_)) {
                            if scan_running.load(std::sync::atomic::Ordering::Acquire) { continue; }
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                        log::info!("kg_watch: change detected, re-scanning...");
                        if let Err(e) = store_clone.scan_codebase(&root).await {
                            log::warn!("kg_watch: scan failed: {e}");
                        }
                    }
                    Ok(Err(e)) => log::warn!("kg_watch: error: {e}"),
                    Err(_) => break,
                }
            }
        });
        if let Ok(mut h) = self.watch_handle.lock() { *h = Some(handle); }
        std::mem::forget(watcher);
    }

    /// Stop file watcher.
    pub fn stop_watch(&self) {
        if let Ok(mut h) = self.watch_handle.lock() {
            if let Some(handle) = h.take() { handle.abort(); log::info!("kg_watch: stopped"); }
        }
    }

    /// Scan git history and write commit/file relationships.
    async fn scan_git_history(&self, git_root: &std::path::Path, project: &Option<String>) -> Result<()> {
        use std::process::Command;
        let now = chrono::Utc::now().to_rfc3339();
        let output = Command::new("git")
            .args(["log", "--name-only", "--pretty=format:COMMIT%x00%H%x00%s%x00%an%x00%ai", "-100", "--diff-filter=AM"])
            .current_dir(git_root).output()?;
        if !output.status.success() { return Ok(()); }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut commit_hash = String::new();
        let mut commit_msg = String::new();
        let mut commit_author = String::new();
        let mut in_files = false;
        for line in stdout.lines() {
            if line.starts_with("COMMIT ") {
                if !commit_hash.is_empty() && in_files {
                    let cid = format!("commit:{}", &commit_hash[..8]);
                    let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'commit',?3,1.0,?4,?5)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(&cid).bind(&commit_hash).bind(&project).bind(&now).execute(&self.pool).await;
                    if !commit_msg.is_empty() {
                        let ms: String = commit_msg.chars().take(80).collect();
                        let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'message',?3,1.0,?4,?5)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(&cid).bind(&ms).bind(&project).bind(&now).execute(&self.pool).await;
                    }
                    if !commit_author.is_empty() {
                        let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'author',?3,1.0,?4,?5)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(&cid).bind(&commit_author).bind(&project).bind(&now).execute(&self.pool).await;
                    }
                }
                let parts: Vec<&str> = line.split(' ').collect();
                if parts.len() >= 5 {
                    commit_hash = parts[1].to_string();
                    commit_msg = parts[2].to_string();
                    commit_author = parts[3].to_string();
                }
                in_files = true;
            } else if in_files && !line.is_empty() {
                let file_path = line.trim();
                if file_path.is_empty() { continue; }
                let cid = format!("commit:{}", &commit_hash[..8]);
                let stem = std::path::Path::new(file_path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,?3,?4,1.0,?5,?6)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(format!("file:{}", stem)).bind("modified_in").bind(&cid).bind(&project).bind(&now).execute(&self.pool).await;
            }
        }
        if !commit_hash.is_empty() && in_files {
            let cid = format!("commit:{}", &commit_hash[..8]);
            let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'commit',?3,1.0,?4,?5)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(&cid).bind(&commit_hash).bind(&project).bind(&now).execute(&self.pool).await;
            if !commit_msg.is_empty() {
                let ms: String = commit_msg.chars().take(80).collect();
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'message',?3,1.0,?4,?5)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(&cid).bind(&ms).bind(&project).bind(&now).execute(&self.pool).await;
            }
            if !commit_author.is_empty() {
                let _ = sqlx::query("INSERT OR IGNORE INTO kg_triples (id,subject,predicate,object,confidence,project,created_at) VALUES (?1,?2,'author',?3,1.0,?4,?5)").bind(format!("cg_{}", uuid::Uuid::new_v4())).bind(&cid).bind(&commit_author).bind(&project).bind(&now).execute(&self.pool).await;
            }
        }
        Ok(())
    }

    pub async fn save(&self, input: NewMemory) -> Result<String> {
        let id = format!("mem_{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        let tags = input
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());
        let files = input
            .files
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());
        sqlx::query("INSERT INTO memories (id,type,content,tags,files,project,source_file,importance,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)")
            .bind(&id).bind(&input.memory_type).bind(&input.content).bind(&tags).bind(&files).bind(&input.project).bind(&input.source_file).bind(3).bind(&now)
            .execute(&self.pool).await?;
        // embedding=NULL — embed-worker picks it up later
        Ok(id)
    }

    async fn init_vec(&self) -> Result<()> {
        // Check if existing embeddings use wrong dimensions
        // Use most common stored dims (not LIMIT 1 — might hit stale row)
        let stored_dims: Option<i64> = sqlx::query_scalar(
            "SELECT embedding_dims FROM memories WHERE embedding IS NOT NULL GROUP BY 1 ORDER BY COUNT(*) DESC LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        let needs_rebuild =
            self.vec_dims > 0 && stored_dims.is_some_and(|d| d as usize != self.vec_dims);
        if needs_rebuild {
            log::info!(
                "init_vec: stored dims != {}, dropping vec0 + clearing embeddings",
                self.vec_dims
            );
            sqlx::query("DROP TABLE IF EXISTS vec_memories")
                .execute(&self.pool)
                .await?;
            sqlx::query(
                "UPDATE memories SET embedding = NULL, embedding_model = NULL, embedding_dims = NULL"
            )
            .execute(&self.pool)
            .await?;
        }

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(embedding float[{dims}])",
            dims = self.vec_dims,
        )))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// sqlite-vec KNN search. Falls back to FTS5 if vec extension not loaded.
    pub async fn search_vec(
        &self,
        query_vec_orig: &[f32],
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let mut query_vec = query_vec_orig.to_vec();
        normalize_l2(&mut query_vec);
        let query_vec = query_vec.as_slice();
        let json_vec: String = serde_json::to_string(&query_vec)?;
        let lim = limit.min(50) as i64;

        #[allow(clippy::type_complexity)]
        let rows: Vec<(String, Option<String>, String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i32>, Option<String>, f64)> =
            if let Some(t) = memory_type {
                sqlx::query_as(
                    "SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, v.distance                  FROM vec_memories v                  JOIN memories m ON m.rowid = v.rowid WHERE m.deleted_at IS NULL AND m.type = ?4 AND v.embedding MATCH ?1 AND v.k = ?2                  ORDER BY v.distance LIMIT ?3",
                )
                .bind(&json_vec).bind(lim).bind(lim).bind(t)
                .fetch_all(&self.pool)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, v.distance                  FROM vec_memories v                  JOIN memories m ON m.rowid = v.rowid WHERE m.deleted_at IS NULL AND v.embedding MATCH ?1 AND v.k = ?2                  ORDER BY v.distance LIMIT ?3",
                )
                .bind(&json_vec).bind(lim).bind(lim)
                .fetch_all(&self.pool)
                .await?
            };

        Ok(rows
            .into_iter()
            .map(
                |(
                    id,
                    mt,
                    content,
                    tags,
                    files,
                    project,
                    source_file,
                    importance,
                    created_at,
                    distance,
                )| {
                    SearchResult {
                        id,
                        memory_type: mt,
                        content,
                        tags: parse_json(&tags),
                        files: parse_json(&files),
                        project,
                        source_file,
                        importance: importance.unwrap_or(3),
                        score: (1.0_f64 - distance.powi(2) / 2.0).max(0.0),
                        created_at: created_at.unwrap_or_default(),
                        embedding: None,
                    }
                },
            )
            .collect())
    }

    /// Hybrid search with Reciprocal Rank Fusion (RRF).
    /// Runs vec0 KNN and FTS5 BM25 concurrently, then merges scores via RRF (k=60).
    pub async fn search(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let k = limit.min(MAX_LIMIT) as usize;

        // Get query embedding upfront (needed for vec0, may be used for fallback)
        let qv = if self.vec_enabled {
            if let Some(ref emb) = self.embedder {
                emb.embed_one(query).await.ok()
            } else {
                None
            }
        } else {
            None
        };

        // Run both search paths concurrently
        let (mut vec_results, fts_results) = if let Some(ref qv) = qv {
            let vec_fut = self.search_vec(qv, limit, memory_type);
            let fts_fut = self.search_fts(query, limit.min(MAX_LIMIT), memory_type);
            let (vr, fr) = tokio::join!(vec_fut, fts_fut);
            let vec_r = vr.unwrap_or_default();
            let fts_r = fr?;
            (vec_r, fts_r)
        } else {
            let fts_r = self
                .search_fts(query, limit.min(MAX_LIMIT), memory_type)
                .await?;
            (vec![], fts_r)
        };

        if vec_results.is_empty() {
            if !fts_results.is_empty() {
                let mut fts_results = fts_results;
                for r in &mut fts_results {
                    r.score = self.decay_score(r.score, &r.created_at);
                }
                log::info!("rrf: FTS5-only ({} results)", fts_results.len());
                return Ok(fts_results);
            }
            if let Some(ref emb) = self.embedder {
                if qv.is_some() {
                    let hybrid = self.search_hybrid(emb, query, limit, memory_type).await?;
                    log::info!(
                        "rrf: cosine rerank fallback ({} results, top={:.3})",
                        hybrid.len(),
                        hybrid.first().map(|r| r.score).unwrap_or(0.0)
                    );
                    return Ok(hybrid);
                }
            }
            return Ok(vec![]);
        }

        // Apply temporal decay to vec_results before merge
        for r in &mut vec_results {
            r.score = self.decay_score(r.score, &r.created_at);
        }

        let merged = rrf_merge(vec_results, fts_results, k, self.rrf_k);
        log::info!("rrf: merged {} results", merged.len());
        Ok(merged)
    }

    /// FTS5-only keyword search.
    async fn search_fts(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let lim = limit.min(MAX_LIMIT) as i64;
        let rows: Vec<SearchRow> = if let Some(t) = memory_type {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?3")
                .bind(query).bind(t).bind(lim).fetch_all(&self.pool).await?
        } else {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?2")
                .bind(query).bind(lim).fetch_all(&self.pool).await?
        };
        Ok(rows
            .into_iter()
            .map(|r| SearchResult {
                id: r.id,
                memory_type: r.memory_type,
                content: r.content,
                tags: parse_json(&r.tags),
                files: parse_json(&r.files),
                project: r.project,
                source_file: r.source_file.clone(),
                importance: r.importance.unwrap_or(3),
                score: 0.5,
                created_at: r.created_at.unwrap_or_default(),
                embedding: r.embedding,
            })
            .collect())
    }

    /// Hybrid search: FTS5 recall + cosine rerank with query embedding.
    pub async fn search_hybrid(
        &self,
        embedder: &crate::embed::embeddings::Embedder,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        // Step 1: Get query embedding from local/remote model
        let query_vec = embedder.embed_one(query).await?;

        // Step 2: FTS5 recall (wider window for reranking)
        let recall_limit = (limit as i64 * RECALL_MULTIPLIER as i64).min(MAX_LIMIT as i64);
        let rows: Vec<SearchRow> = if let Some(t) = memory_type {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?3")
                .bind(query).bind(t).bind(recall_limit).fetch_all(&self.pool).await?
        } else {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.source_file, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.deleted_at IS NULL ORDER BY rank LIMIT ?2")
                .bind(query).bind(recall_limit).fetch_all(&self.pool).await?
        };

        // Step 3: Cosine rerank (only rows with embeddings)
        let mut results: Vec<SearchResult> = Vec::new();
        let mut bm25_only: Vec<SearchResult> = Vec::new();

        for r in rows {
            if let Some(ref emb) = r.embedding {
                if let Some(emb_vec) = bytes_to_f32_vec(emb) {
                    let cosine = cosine_similarity(&query_vec, &emb_vec);
                    results.push(SearchResult {
                        id: r.id,
                        memory_type: r.memory_type,
                        content: r.content,
                        tags: parse_json(&r.tags),
                        files: parse_json(&r.files),
                        project: r.project,
                        source_file: r.source_file.clone(),
                        importance: r.importance.unwrap_or(3),
                        score: cosine as f64,
                        created_at: r.created_at.unwrap_or_default(),
                        embedding: Some(emb.clone()),
                    });
                    continue;
                }
            }
            // Fallback: BM25-only (no embedding yet)
            bm25_only.push(SearchResult {
                id: r.id,
                memory_type: r.memory_type,
                content: r.content,
                tags: parse_json(&r.tags),
                files: parse_json(&r.files),
                project: r.project,
                source_file: r.source_file.clone(),
                importance: r.importance.unwrap_or(3),
                score: 0.5,
                created_at: r.created_at.unwrap_or_default(),
                embedding: r.embedding.clone(),
            });
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit as usize);

        // Append BM25-only results for padding
        if results.len() < limit as usize {
            let remaining = limit as usize - results.len();
            results.extend(bm25_only.into_iter().take(remaining));
        }

        Ok(results)
    }

    pub async fn list(&self, limit: u8, memory_type: Option<&str>, offset: u32) -> Result<Vec<Memory>> {
        let rows: Vec<MemoryRow> = if let Some(t) = memory_type {
            sqlx::query_as("SELECT id,type,content,tags,files,project,source_file,importance,created_at,updated_at FROM memories WHERE type=?1 AND deleted_at IS NULL ORDER BY created_at DESC LIMIT ?2 OFFSET ?3")
                .bind(t).bind(limit.min(MAX_LIMIT) as i64).bind(offset).fetch_all(&self.pool).await?
        } else {
            sqlx::query_as("SELECT id,type,content,tags,files,project,source_file,importance,created_at,updated_at FROM memories WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT ?1 OFFSET ?2")
                .bind(limit.min(MAX_LIMIT) as i64).bind(offset).fetch_all(&self.pool).await?
        };
        Ok(rows
            .into_iter()
            .map(|r| Memory {
                id: r.id,
                memory_type: r.memory_type,
                content: r.content,
                tags: parse_json(&r.tags),
                files: parse_json(&r.files),
                project: r.project,
                source_file: r.source_file.clone(),
                importance: r.importance.unwrap_or(3),
                created_at: r.created_at.unwrap_or_default(),
                updated_at: r.updated_at.unwrap_or_default(),
                session_id: None,
            })
            .collect())
    }

    pub async fn forget(&self, id: &str) -> Result<()> {
        // Also delete from vec0 if enabled (FTS TRIGGER handles FTS cleanup automatically)
        if self.vec_enabled {
            if let Ok(Some(rid)) = sqlx::query_scalar::<_, i64>(
                "SELECT rowid FROM memories WHERE id = ?1",
            )
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            {
                sqlx::query("DELETE FROM vec_memories WHERE rowid = ?1")
                    .bind(rid)
                    .execute(&self.pool)
                    .await?;
            }
        }
        // Hard delete. FTS TRIGGER removes from index automatically.
        sqlx::query("DELETE FROM memories WHERE id=?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn wipe(&self) -> Result<()> {
        sqlx::query("DELETE FROM memories")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM memories_fts")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM sessions")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM observations")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Multiply score by e^(-days/half_life) for temporal decay.
    fn decay_score(&self, score: f64, created_at: &str) -> f64 {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(created_at) {
            let fixed: chrono::DateTime<Utc> = dt.into();
            let age_days = (Utc::now() - fixed).num_hours() as f64 / 24.0;
            if age_days > 0.0 {
                score * (-age_days / self.half_life_days).exp()
            } else {
                score
            }
        } else {
            score
        }
    }

    pub async fn stats(&self) -> Result<DbStats> {
        let mc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE deleted_at IS NULL")
            .fetch_one(&self.pool)
            .await?;
        let we: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memories WHERE deleted_at IS NULL AND embedding IS NOT NULL",
        )
        .fetch_one(&self.pool)
        .await?;
        let sc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);
        let oc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observations")
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

        // Type distribution
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT COALESCE(type,'(none)') as t, COUNT(*) as c FROM memories WHERE deleted_at IS NULL GROUP BY type ORDER BY c DESC",
        )
        .fetch_all(&self.pool).await?;

        // Current model info
        let model: Option<(String, i64)> = sqlx::query_as(
            "SELECT embedding_model, embedding_dims FROM memories WHERE embedding IS NOT NULL AND deleted_at IS NULL LIMIT 1",
        )
        .fetch_optional(&self.pool).await?;
        let model_info = model.map(|(m, d)| format!("{m} ({d}d)"));

        let db_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        let schema_version: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(version), 1) FROM _schema_version")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(1);
        Ok(DbStats {
            memory_count: mc,
            with_embedding: we,
            type_counts: rows,
            model_info,
            session_count: sc,
            observation_count: oc,
            db_path: self.path.to_string_lossy().to_string(),
            db_size_bytes: db_size,
            schema_version,
            vec0_enabled: self.vec_enabled,
            tool_count: 6,
        })
    }

    // ─── embed worker (public, called from mcp server) ──────────────────────
    /// Deduplicate memories by content+type, keeping the oldest (by created_at).
    /// Also VACUUM to reclaim disk space.
    pub async fn compact(&self, mode: &str, threshold: f32) -> Result<(usize, usize)> {
        if mode == "semantic" {
            return self.compact_semantic(threshold).await;
        }

        let before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        sqlx::query(
            "DELETE FROM memories WHERE rowid NOT IN (SELECT MIN(rowid) FROM memories GROUP BY content, COALESCE(type,''))",
        )
        .execute(&self.pool).await?;

        let after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        self.rebuild_fts().await?;
        sqlx::query("VACUUM").execute(&self.pool).await?;

        Ok(((before - after) as usize, after as usize))
    }

    async fn compact_semantic(&self, threshold: f32) -> Result<(usize, usize)> {
        let before: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;

        #[derive(sqlx::FromRow)]
        struct EmbedRow {
            id: String,
            embedding: Vec<u8>,
        }
        let rows: Vec<EmbedRow> = sqlx::query_as(
            "SELECT id, embedding FROM memories WHERE embedding IS NOT NULL AND deleted_at IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        if rows.len() < 2 {
            return Ok((0, before as usize));
        }

        let vecs: Vec<Vec<f32>> = rows
            .iter()
            .filter_map(|r| bytes_to_f32_vec(&r.embedding))
            .collect();
        let n = vecs.len();
        let mut parent: Vec<usize> = (0..n).collect();

        for i in 0..n {
            for j in (i + 1)..n {
                if cosine_similarity(&vecs[i], &vecs[j]) > threshold {
                    let mut ri = i;
                    while parent[ri] != ri {
                        ri = parent[ri];
                    }
                    let mut rj = j;
                    while parent[rj] != rj {
                        rj = parent[rj];
                    }
                    if ri != rj {
                        parent[ri] = rj;
                    }
                }
            }
        }

        use std::collections::HashMap;
        let mut groups: HashMap<usize, Vec<&str>> = HashMap::new();
        for (i, row) in rows.iter().enumerate() {
            let mut root = i;
            while parent[root] != root {
                root = parent[root];
            }
            groups.entry(root).or_default().push(&row.id);
        }

        for ids in groups.values() {
            if ids.len() > 1 {
                for id in &ids[1..] {
                    sqlx::query("DELETE FROM memories WHERE id = ?1")
                        .bind(id)
                        .execute(&self.pool)
                        .await?;
                }
            }
        }

        let after: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE deleted_at IS NULL")
                .fetch_one(&self.pool)
                .await?;
        self.rebuild_fts().await?;
        sqlx::query("VACUUM").execute(&self.pool).await?;
        Ok(((before - after) as usize, after as usize))
    }

    async fn rebuild_fts(&self) -> Result<()> {
        sqlx::query("DELETE FROM memories_fts")
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO memories_fts(rowid, content) SELECT rowid, content FROM memories")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Poll for rows without embeddings and compute them via remote API.
    pub async fn embed_pending(
        &self,
        embedder: &crate::embed::embeddings::Embedder,
        model_name: &str,
        dims: usize,
        batch_size: usize,
    ) -> Result<usize> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, content FROM memories WHERE deleted_at IS NULL AND (embedding IS NULL OR embedding_model IS NOT ?2 OR embedding_dims IS NOT ?3) ORDER BY embedding IS NULL DESC, created_at ASC LIMIT ?1",
        )
        .bind(batch_size as i64)
        .bind(model_name)
        .bind(dims as i64)
        .fetch_all(&self.pool)
        .await?;

        if rows.is_empty() {
            return Ok(0);
        }

        let ids: Vec<&str> = rows.iter().map(|(id, _)| id.as_str()).collect();
        let texts: Vec<&str> = rows.iter().map(|(_, c)| c.as_str()).collect();

        let vectors = embedder.embed_batch(&texts).await?;
        log::info!(
            "embed_pending: got {} vectors, dim={} from model={}",
            vectors.len(),
            vectors.first().map(|v| v.len()).unwrap_or(0),
            model_name
        );
        if vectors.len() != rows.len() {
            anyhow::bail!(
                "embed_batch returned {} vectors for {} inputs",
                vectors.len(),
                rows.len()
            );
        }

        let dims_i64 = dims as i64;

        for ((id, _), vec) in rows.iter().zip(vectors.iter()) {
            let blob: Vec<u8> = vec.iter().flat_map(|f| f32::to_le_bytes(*f)).collect();
            sqlx::query("UPDATE memories SET embedding=?1, embedding_model=?2, embedding_dims=?3 WHERE id=?4")
                .bind(&blob)
                .bind(model_name)
                .bind(dims_i64)
                .bind(id)
                .execute(&self.pool)
                .await?;

            // Also insert into vec0 for KNN search
            if self.vec_enabled {
                if let Ok(rowid) =
                    sqlx::query_scalar::<_, i64>("SELECT rowid FROM memories WHERE id = ?1")
                        .bind(id)
                        .fetch_one(&self.pool)
                        .await
                {
                    let mut vec_clone = vec.clone();
                    normalize_l2(&mut vec_clone);
                    let json_vec = serde_json::to_string(&vec_clone).unwrap_or_default();
                    sqlx::query(
                        "INSERT OR REPLACE INTO vec_memories(rowid, embedding) VALUES (?1, ?2)",
                    )
                    .bind(rowid)
                    .bind(&json_vec)
                    .execute(&self.pool)
                    .await?;
                }
            }
        }

        Ok(ids.len())
    }

    /// Gracefully shut down the store: flush WAL, close pool.
    pub async fn shutdown(self) {
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await;
        self.pool.close().await;
    }
}

// ─── Row types ──────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct SearchRow {
    id: String,
    #[sqlx(rename = "type")]
    memory_type: Option<String>,
    content: String,
    tags: Option<String>,
    files: Option<String>,
    project: Option<String>,
    source_file: Option<String>,
    importance: Option<i32>,
    created_at: Option<String>,
    embedding: Option<Vec<u8>>,
}
#[derive(sqlx::FromRow)]
struct MemoryRow {
    id: String,
    #[sqlx(rename = "type")]
    memory_type: Option<String>,
    content: String,
    tags: Option<String>,
    files: Option<String>,
    project: Option<String>,
    source_file: Option<String>,
    importance: Option<i32>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// ─── Vector math ─────────────────────────────────────────────────────────────

fn normalize_l2(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

fn bytes_to_f32_vec(bytes: &[u8]) -> Option<Vec<f32>> {
    if bytes.len() % 4 != 0 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (dot, na, nb) = a
        .iter()
        .zip(b.iter())
        .fold((0.0f32, 0.0f32, 0.0f32), |(d, na, nb), (&x, &y)| {
            (d + x * y, na + x * x, nb + y * y)
        });
    let denom = (na.sqrt() * nb.sqrt()).max(1e-10);
    (dot / denom).clamp(-1.0, 1.0)
}

fn parse_json(val: &Option<String>) -> Option<Vec<String>> {
    match val {
        Some(s) if !s.is_empty() => serde_json::from_str(s).ok(),
        _ => None,
    }
}

/// Detect project name from path or git for code scanning.
fn detect_project_git_root(root: &std::path::Path) -> Option<std::path::PathBuf> {
    std::process::Command::new("git").args(["rev-parse", "--show-toplevel"]).current_dir(root).output().ok().and_then(|o| if o.status.success() { let p = String::from_utf8_lossy(&o.stdout).trim().to_string(); if p.is_empty() { None } else { Some(std::path::PathBuf::from(p)) } } else { None })
}

fn detect_project_for_scan(root: &std::path::Path) -> Option<String> {
    // Try git first
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
    // Fallback: use directory name
    root.file_name().map(|n| n.to_string_lossy().to_string())
}

// ─── Re-embed integration tests ──────────────────────────────────────────────
#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use tempfile::tempdir;

    fn fake_blob(dims: usize, val: f32) -> Vec<u8> {
        let v: Vec<f32> = vec![val; dims];
        v.iter().flat_map(|f| f32::to_le_bytes(*f)).collect()
    }

    #[tokio::test]
    async fn test_reembed_flow() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("memory.db");

        // Raw SQLite pool (no vec0 extension) to test re-embed SQL logic
        let opts = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opts).await.unwrap();

        sqlx::query("CREATE TABLE IF NOT EXISTS memories (id TEXT PRIMARY KEY, type TEXT, content TEXT NOT NULL, tags TEXT, files TEXT, project TEXT, source_file TEXT, importance INTEGER DEFAULT 3, embedding BLOB, embedding_model TEXT, embedding_dims INTEGER, created_at TEXT, updated_at TEXT, deleted_at TEXT)")
            .execute(&pool).await.unwrap();

        async fn ins(pool: &sqlx::SqlitePool, content: &str, blob: &[u8], model: &str, dims: i64) -> String {
            let id = format!("mem_{}", uuid::Uuid::new_v4());
            let now = chrono::Utc::now().to_rfc3339();
            sqlx::query("INSERT INTO memories (id,type,content,importance,created_at,updated_at) VALUES (?1,'fact',?2,3,?3,?3)")
                .bind(&id).bind(content).bind(&now)
                .execute(pool).await.unwrap();
            if !blob.is_empty() {
                sqlx::query("UPDATE memories SET embedding=?1, embedding_model=?2, embedding_dims=?3 WHERE id=?4")
                    .bind(blob).bind(model).bind(dims).bind(&id)
                    .execute(pool).await.unwrap();
            }
            id
        }

        // ─── init_vec dimension-check SQL ───
        for _ in 0..3 {
            ins(&pool, "64d", &fake_blob(64, 0.1), "m:64d", 64).await;
        }
        let sd: Option<i64> = sqlx::query_scalar(
            "SELECT embedding_dims FROM memories WHERE embedding IS NOT NULL GROUP BY 1 ORDER BY COUNT(*) DESC LIMIT 1"
        ).fetch_optional(&pool).await.unwrap();
        assert_eq!(sd, Some(64));
        assert!(sd.is_some_and(|d| d as usize != 128), "64 vs 128 should trigger rebuild");
        assert!(!sd.is_some_and(|d| d as usize != 64), "64 vs 64 should NOT trigger rebuild");

        // NULL embeddings ignored by GROUP BY
        let sd2: Option<i64> = sqlx::query_scalar(
            "SELECT embedding_dims FROM memories WHERE embedding IS NOT NULL GROUP BY 1 ORDER BY COUNT(*) DESC LIMIT 1"
        ).fetch_optional(&pool).await.unwrap();
        assert_eq!(sd2, Some(64), "NULL emb should not affect stored_dims");

        // ─── embed_pending SQL ───
        let id_null  = ins(&pool, "pending", &[], "", 0).await;
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        let id_fresh = ins(&pool, "fresh", &fake_blob(64,0.1), "curr:64d", 64).await;
        let id_stale = ins(&pool, "stale", &fake_blob(64,0.1), "old:64d", 64).await;
        let id_wd    = ins(&pool, "wd", &fake_blob(32,0.1), "old:32d", 32).await;

        let pending: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, content FROM memories WHERE deleted_at IS NULL AND (embedding IS NULL OR embedding_model IS NOT ?2 OR embedding_dims IS NOT ?3) ORDER BY embedding IS NULL DESC, created_at ASC LIMIT ?1"
        ).bind(100i64).bind("curr:64d").bind(64i64)
         .fetch_all(&pool).await.unwrap();

        let pids: Vec<&str> = pending.iter().map(|(id,_)| id.as_str()).collect();
        assert!(pids.contains(&id_null.as_str()), "NULL embed must be pending");
        assert!(!pids.contains(&id_fresh.as_str()), "fresh must NOT be pending");
        assert!(pids.contains(&id_stale.as_str()), "stale model must be pending");
        assert!(pids.contains(&id_wd.as_str()), "wrong dims must be pending");
        assert_eq!(pids[0], id_null.as_str(), "NULL should sort first by created_at");
    }
}
