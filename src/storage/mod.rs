//! SQLite storage via sqlx (async, Send+Sync, connection pool).
//!
//! Sub-modules:
//! - `schema`:  table/index/trigger initialization, vec0 virtual table, FTS rebuild
//! - `search`:  RRF fusion, vec0 KNN, FTS5 BM25, cosine rerank, row types, vector math
//! - `kg`:      codebase scanning, git history import, SPO triple CRUD

mod schema;
pub(crate) mod search;
mod kg;

use anyhow::Result;
use chrono::Utc;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::embed::embeddings::Embedder;
use crate::knowledge_graph::KnowledgeGraph;
use crate::types::{DbStats, Memory, NewMemory, SearchResult};

// Re-export types and helpers used by CLI modules
pub(crate) use search::{MemoryRow, SearchRow, bytes_to_f32_vec, cosine_similarity, parse_json};
pub(crate) use search::normalize_l2;
pub(crate) use search::MAX_LIMIT;

// Embedded sqlite-vec extension for the current platform.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const VEC_EXT_BYTES: &[u8] = include_bytes!("../../ext/vec0-linux-x86_64.so");
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
const VEC_EXT_BYTES: &[u8] = &[];

#[derive(Clone)]
pub struct Store {
    pub(crate) pool: SqlitePool,
    path: PathBuf,
    embedder: Option<Arc<Embedder>>,
    pub(crate) graph: KnowledgeGraph,
    pub(crate) scan_running: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) watch_handle: Arc<std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
    pub(crate) scan_result: Arc<std::sync::Mutex<Option<String>>>,
    pub(crate) tasks: Arc<crate::mcp::tasks::TaskManager>,
    vec_enabled: bool,
    vec_dims: usize,
    rrf_k: f64,
    half_life_days: f64,
    _default_limit: u8,
    _list_limit: u8,
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
            tasks: crate::mcp::tasks::TaskManager::new(),
            vec_enabled,
            vec_dims: dims,
            rrf_k: cfg.search.rrf_k,
            half_life_days: cfg.search.half_life_days,
            _default_limit: cfg.search.default_limit,
            _list_limit: cfg.search.list_limit,
        };
        store.initialize().await?;
        if vec_enabled {
            store.init_vec().await?;
        }
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
        schema::initialize(&self.pool).await
    }

    /// Add a SPO triple to the knowledge graph.
    #[allow(dead_code)]
    pub async fn add_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        confidence: f32,
        source_memory_id: Option<String>,
        project: Option<String>,
    ) -> Result<String> {
        kg::add_triple(
            &self.pool,
            &self.graph,
            subject,
            predicate,
            object,
            confidence,
            source_memory_id,
            project,
        )
        .await
    }

    /// Scan a codebase directory with tree-sitter and import results into KG.
    pub async fn scan_codebase(&self, root: &std::path::Path) -> Result<(usize, usize)> {
        let result = kg::scan_codebase(&self.pool, &self.graph, root).await?;
        if let Ok(mut r) = self.scan_result.lock() {
            *r = Some(format!(
                "kg_scan: {} symbols, {} relations",
                result.0, result.1
            ));
        }
        Ok(result)
    }

    pub fn start_watch(&self, root: &std::path::Path) {
        use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
        use std::sync::mpsc;
        let root = root.to_path_buf();
        let scan_running = self.scan_running.clone();
        let store_clone = self.clone();
        let (tx, rx) = mpsc::channel::<Result<notify::Event, notify::Error>>();
        let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
            Ok(w) => w,
            Err(e) => {
                log::warn!("kg_watch: failed to create watcher: {e}");
                return;
            }
        };
        if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
            log::warn!("kg_watch: failed to watch {root:?}: {e}");
            return;
        }
        let handle = tokio::spawn(async move {
            log::info!("kg_watch: watching {root:?} for changes");
            loop {
                match rx.recv() {
                    Ok(Ok(event)) => {
                        let relevant = event.paths.iter().any(|p| {
                            p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                                matches!(
                                    e,
                                    "rs" | "py"
                                        | "ts"
                                        | "tsx"
                                        | "js"
                                        | "jsx"
                                        | "java"
                                        | "go"
                                        | "rb"
                                        | "php"
                                        | "swift"
                                        | "kt"
                                        | "c"
                                        | "cpp"
                                        | "h"
                                        | "hpp"
                                        | "cs"
                                        | "scala"
                                        | "sh"
                                        | "bash"
                                        | "zsh"
                                )
                            })
                        });
                        if !relevant {
                            continue;
                        }
                        if matches!(event.kind, EventKind::Modify(_)) {
                            if scan_running.load(std::sync::atomic::Ordering::Acquire) {
                                continue;
                            }
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
        if let Ok(mut h) = self.watch_handle.lock() {
            *h = Some(handle);
        }
        std::mem::forget(watcher);
    }
    /// Stop file watcher.
    #[allow(dead_code)]
    pub fn stop_watch(&self) {
        kg::stop_watch(&self.watch_handle);
    }

    /// Scan git history and write commit/file relationships.
    async fn scan_git_history(
        &self,
        git_root: &std::path::Path,
        project: &Option<String>,
    ) -> Result<()> {
        kg::scan_git_history(&self.pool, git_root, project).await
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
        let result = sqlx::query("INSERT OR IGNORE INTO memories (id,type,content,tags,files,project,source_file,importance,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)")
            .bind(&id).bind(&input.memory_type).bind(&input.content).bind(&tags).bind(&files).bind(&input.project).bind(&input.source_file).bind(3).bind(&now)
            .execute(&self.pool).await?;
        if result.rows_affected() == 0 {
            // Duplicate content+type — return existing ID
            let existing: String = sqlx::query_scalar(
                "SELECT id FROM memories WHERE content = ?1 AND type = ?2 AND deleted_at IS NULL"
            )
            .bind(&input.content)
            .bind(&input.memory_type)
            .fetch_one(&self.pool)
            .await?;
            return Ok(existing);
        }
        Ok(id)
    }
    async fn init_vec(&self) -> Result<()> {
        schema::init_vec(&self.pool, self.vec_dims).await
    }

    /// sqlite-vec KNN search. Falls back to FTS5 if vec extension not loaded.
    pub async fn search_vec(
        &self,
        query_vec_orig: &[f32],
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        search::search_vec(&self.pool, query_vec_orig, limit, memory_type).await
    }

    /// Hybrid search with Reciprocal Rank Fusion (RRF).
    /// Runs vec0 KNN and FTS5 BM25 concurrently, then merges scores via RRF (k=60).
    pub async fn search(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let decay = |score: f64, created_at: &str| self.decay_score(score, created_at);
        search::search_rrf(
            &self.pool,
            &self.embedder,
            self.vec_enabled,
            query,
            limit,
            memory_type,
            self.rrf_k,
            decay,
        )
        .await
    }

    /// FTS5-only keyword search.
    async fn search_fts(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        search::search_fts(&self.pool, query, limit, memory_type).await
    }

    /// Hybrid search: FTS5 recall + cosine rerank with query embedding.
    pub async fn search_hybrid(
        &self,
        embedder: &crate::embed::embeddings::Embedder,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        search::search_hybrid(&self.pool, embedder, query, limit, memory_type).await
    }

    pub async fn list(
        &self,
        limit: u8,
        memory_type: Option<&str>,
        offset: u32,
    ) -> Result<Vec<Memory>> {
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
    pub async fn forget(&self, id: &str) -> Result<()> {
        // Also delete from vec0 if enabled (FTS TRIGGER handles FTS cleanup automatically)
        if self.vec_enabled {
            if let Ok(Some(rid)) =
                sqlx::query_scalar::<_, i64>("SELECT rowid FROM memories WHERE id = ?1")
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
        schema::rebuild_fts(&self.pool).await
    }

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

    pub async fn shutdown(self) {
        let _ = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&self.pool)
            .await;
        self.pool.close().await;
    }
}

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

        async fn ins(
            pool: &sqlx::SqlitePool,
            content: &str,
            blob: &[u8],
            model: &str,
            dims: i64,
        ) -> String {
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
        assert!(
            sd.is_some_and(|d| d as usize != 128),
            "64 vs 128 should trigger rebuild"
        );
        assert!(
            !sd.is_some_and(|d| d as usize != 64),
            "64 vs 64 should NOT trigger rebuild"
        );

        // NULL embeddings ignored by GROUP BY
        let sd2: Option<i64> = sqlx::query_scalar(
            "SELECT embedding_dims FROM memories WHERE embedding IS NOT NULL GROUP BY 1 ORDER BY COUNT(*) DESC LIMIT 1"
        ).fetch_optional(&pool).await.unwrap();
        assert_eq!(sd2, Some(64), "NULL emb should not affect stored_dims");

        // ─── embed_pending SQL ───
        let id_null = ins(&pool, "pending", &[], "", 0).await;
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        let id_fresh = ins(&pool, "fresh", &fake_blob(64, 0.1), "curr:64d", 64).await;
        let id_stale = ins(&pool, "stale", &fake_blob(64, 0.1), "old:64d", 64).await;
        let id_wd = ins(&pool, "wd", &fake_blob(32, 0.1), "old:32d", 32).await;

        let pending: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, content FROM memories WHERE deleted_at IS NULL AND (embedding IS NULL OR embedding_model IS NOT ?2 OR embedding_dims IS NOT ?3) ORDER BY embedding IS NULL DESC, created_at ASC LIMIT ?1"
        ).bind(100i64).bind("curr:64d").bind(64i64)
         .fetch_all(&pool).await.unwrap();

        let pids: Vec<&str> = pending.iter().map(|(id, _)| id.as_str()).collect();
        assert!(
            pids.contains(&id_null.as_str()),
            "NULL embed must be pending"
        );
        assert!(
            !pids.contains(&id_fresh.as_str()),
            "fresh must NOT be pending"
        );
        assert!(
            pids.contains(&id_stale.as_str()),
            "stale model must be pending"
        );
        assert!(pids.contains(&id_wd.as_str()), "wrong dims must be pending");
        assert_eq!(
            pids[0],
            id_null.as_str(),
            "NULL should sort first by created_at"
        );
    }

    #[tokio::test]
    async fn test_scan_git_history() {
        use std::process::Command;
        let tmp = tempfile::tempdir().unwrap();
        let git_dir = tmp.path().join("test_repo");
        std::fs::create_dir(&git_dir).unwrap();

        // Initialize git repo
        Command::new("git")
            .args(["init"])
            .current_dir(&git_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&git_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Tester"])
            .current_dir(&git_dir)
            .output()
            .unwrap();

        // Create first file and commit
        std::fs::write(git_dir.join("hello.rs"), "fn hello() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&git_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "feat: add hello function"])
            .current_dir(&git_dir)
            .output()
            .unwrap();

        // Create second file and commit
        std::fs::write(git_dir.join("main.rs"), "fn main() {}").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(&git_dir)
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "feat: add main entry"])
            .current_dir(&git_dir)
            .output()
            .unwrap();

        // Create Store — it creates kg_triples table automatically in its own DB
        let mut cfg = crate::config::Config::load(None, None);
        cfg.db_dir = Some(tmp.path().to_path_buf());
        cfg.embedding.backend = crate::config::EmbeddingBackend::None;
        cfg.knowledge_graph = crate::config::KnowledgeGraphConfig { enabled: true };

        let store = crate::storage::Store::open(&cfg, None).await.unwrap();

        // Run git history scan
        store.scan_git_history(&git_dir, &None).await.unwrap();

        // Query through store's pool
        let pool = store.pool.clone();
        let rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT subject, predicate, object FROM kg_triples WHERE subject LIKE 'commit:%'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();

        assert!(!rows.is_empty(), "scan_git_history should produce triples");

        // Verify commit messages
        let messages: Vec<&str> = rows
            .iter()
            .filter_map(|(_, p, o)| {
                if p == "message" {
                    Some(o.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(
            messages.contains(&"feat: add hello function"),
            "should have first commit message"
        );
        assert!(
            messages.contains(&"feat: add main entry"),
            "should have second commit message"
        );

        // Verify author
        let authors: Vec<&str> = rows
            .iter()
            .filter_map(|(_, p, o)| {
                if p == "author" {
                    Some(o.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(authors.contains(&"Tester"), "should have author Tester");

        // Verify file relations
        let file_rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT subject, predicate, object FROM kg_triples WHERE subject LIKE 'file:%'",
        )
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!file_rows.is_empty(), "should have file triples");

        pool.close().await;
    }
}
