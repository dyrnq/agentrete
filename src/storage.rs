//! SQLite storage via sqlx (async, Send+Sync, connection pool).

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::embed::embeddings::Embedder;
use crate::types::{DbStats, Memory, NewMemory, SearchResult};

#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
    path: PathBuf,
    embedder: Option<Arc<Embedder>>,
}

impl Store {
    pub async fn open(cfg: &crate::config::Config, embedder: Option<Embedder>) -> Result<Self> {
        let path = cfg.db_dir().join("memory.db");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db_url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&db_url).await?;
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA busy_timeout=5000")
            .execute(&pool)
            .await?;
        let store = Self {
            pool,
            path,
            embedder: embedder.map(Arc::new),
        };
        store.initialize().await?;
        Ok(store)
    }

    async fn initialize(&self) -> Result<()> {
        sqlx::query("CREATE TABLE IF NOT EXISTS _schema_version (version INTEGER PRIMARY KEY, migrated_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS memories (id TEXT PRIMARY KEY, type TEXT, content TEXT NOT NULL, tags TEXT, files TEXT, project TEXT, importance REAL DEFAULT 0.5, embedding BLOB, embedding_model TEXT, embedding_dims INTEGER, created_at TEXT, updated_at TEXT)").execute(&self.pool).await?;
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_embed_null ON memories(embedding) WHERE embedding IS NULL").execute(&self.pool).await?;
        sqlx::query("CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content, content_rowid='rowid', tokenize='unicode61')").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, data TEXT, metadata TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS observations (id TEXT PRIMARY KEY, content TEXT, tool_name TEXT, session_id TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
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
        sqlx::query("INSERT INTO memories (id,type,content,tags,files,project,importance,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)")
            .bind(&id).bind(&input.memory_type).bind(&input.content).bind(&tags).bind(&files).bind(&input.project).bind(0.5).bind(&now)
            .execute(&self.pool).await?;
        let rowid: i64 = sqlx::query_scalar("SELECT rowid FROM memories WHERE id=?1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await?;
        sqlx::query("INSERT INTO memories_fts(rowid,content) VALUES (?1,?2)")
            .bind(rowid)
            .bind(&input.content)
            .execute(&self.pool)
            .await?;
        // embedding=NULL — embed-worker picks it up later
        Ok(id)
    }

    /// Search — auto-selects hybrid if embedder is available, falls back to FTS5.
    pub async fn search(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        if let Some(ref emb) = self.embedder {
            return self.search_hybrid(emb, query, limit, memory_type).await;
        }
        self.search_fts(query, limit, memory_type).await
    }

    /// FTS5-only keyword search.
    async fn search_fts(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let lim = limit.min(100) as i64;
        let rows: Vec<SearchRow> = if let Some(t) = memory_type {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 ORDER BY rank LIMIT ?3")
                .bind(query).bind(t).bind(lim).fetch_all(&self.pool).await?
        } else {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 ORDER BY rank LIMIT ?2")
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
                importance: r.importance.unwrap_or(0.5),
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
        let recall_limit = (limit as i64 * 3).min(100);
        let rows: Vec<SearchRow> = if let Some(t) = memory_type {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 ORDER BY rank LIMIT ?3")
                .bind(query).bind(t).bind(recall_limit).fetch_all(&self.pool).await?
        } else {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at, m.embedding FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 ORDER BY rank LIMIT ?2")
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
                        importance: r.importance.unwrap_or(0.5),
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
                importance: r.importance.unwrap_or(0.5),
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

    pub async fn list(&self, limit: u8) -> Result<Vec<Memory>> {
        let rows: Vec<MemoryRow> = sqlx::query_as("SELECT id,type,content,tags,files,project,importance,created_at,updated_at FROM memories ORDER BY created_at DESC LIMIT ?1")
            .bind(limit.min(100) as i64).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| Memory {
                id: r.id,
                memory_type: r.memory_type,
                content: r.content,
                tags: parse_json(&r.tags),
                files: parse_json(&r.files),
                project: r.project,
                importance: r.importance.unwrap_or(0.5),
                created_at: r.created_at.unwrap_or_default(),
                updated_at: r.updated_at.unwrap_or_default(),
                session_id: None,
            })
            .collect())
    }

    pub async fn forget(&self, id: &str) -> Result<()> {
        // Delete from FTS5 index first (by rowid)
        let rowid: Option<i64> = sqlx::query_scalar("SELECT rowid FROM memories WHERE id=?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;
        if let Some(rid) = rowid {
            sqlx::query("DELETE FROM memories_fts WHERE rowid=?1")
                .bind(rid)
                .execute(&self.pool)
                .await?;
        }
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

    pub async fn stats(&self) -> Result<DbStats> {
        let mc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories")
            .fetch_one(&self.pool)
            .await?;
        let we: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE embedding IS NOT NULL")
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
            "SELECT COALESCE(type,'(none)') as t, COUNT(*) as c FROM memories GROUP BY type ORDER BY c DESC",
        )
        .fetch_all(&self.pool).await?;

        // Current model info
        let model: Option<(String, i64)> = sqlx::query_as(
            "SELECT embedding_model, embedding_dims FROM memories WHERE embedding IS NOT NULL LIMIT 1",
        )
        .fetch_optional(&self.pool).await?;
        let model_info = model.map(|(m, d)| format!("{m} ({d}d)"));

        let db_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        Ok(DbStats {
            memory_count: mc,
            with_embedding: we,
            type_counts: rows,
            model_info,
            session_count: sc,
            observation_count: oc,
            db_path: self.path.to_string_lossy().to_string(),
            db_size_bytes: db_size,
        })
    }

    // ─── embed worker (public, called from mcp server) ──────────────────────

    /// Poll for rows without embeddings and compute them via remote API.
    pub async fn embed_pending(
        &self,
        embedder: &crate::embed::embeddings::Embedder,
        model_name: &str,
        dims: usize,
        batch_size: usize,
    ) -> Result<usize> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, content FROM memories WHERE embedding IS NULL OR embedding_model IS NOT ?2 OR embedding_dims IS NOT ?3 ORDER BY embedding IS NULL DESC, created_at ASC LIMIT ?1",
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
        eprintln!(
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
        }

        Ok(ids.len())
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
    importance: Option<f64>,
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
    importance: Option<f64>,
    created_at: Option<String>,
    updated_at: Option<String>,
}

// ─── Vector math ─────────────────────────────────────────────────────────────

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
