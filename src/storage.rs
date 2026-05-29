//! SQLite storage layer for agentrete.
//!
//! Uses rusqlite with WAL mode for concurrent reads.
//! Vector embeddings stored as BLOB (little-endian f32 array).
//! FTS5 for BM25 full-text search.
//! Model migration: on startup detects dimension mismatch, triggers reindex.

use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};
use tokio::sync::watch;
use uuid::Uuid;

use crate::types::{DbStats, Memory, NewMemory, SearchResult};

pub struct Store {
    conn: Connection,
    path: PathBuf,
    config: crate::config::Config,
    embedder: std::sync::OnceLock<crate::embed::embeddings::Embedder>,
    reindexing: Arc<AtomicBool>,
    reindex_ready: watch::Receiver<bool>,
}

impl Store {
    pub async fn open(cfg: &crate::config::Config) -> Result<Self> {
        let path = cfg.db_dir().join("memory.db");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let conn = Connection::open(&path)
            .with_context(|| format!("Failed to open SQLite at {}", path.display()))?;

        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;").ok();
        // Load sqlite-vec extension
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        let (tx, rx) = watch::channel(true);

        let store = Self {
            conn,
            path,
            config: cfg.clone(),
            embedder: std::sync::OnceLock::new(),
            reindexing: Arc::new(AtomicBool::new(false)),
            reindex_ready: rx,
        };
        store.initialize()?;

        // Detect model mismatch → synchronous reindex
        if store.needs_reindex()? {
            store.reindexing.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = tx.send(false);
            eprintln!("Model mismatch detected — reindexing...");
            let count = store.reindex().await?;
            eprintln!("Reindexed {} memories.", count);
            store.reindexing.store(false, std::sync::atomic::Ordering::SeqCst);
            let _ = tx.send(true);
        }

        Ok(store)
    }

    fn initialize(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _schema_version (
                version     INTEGER PRIMARY KEY,
                migrated_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS memories (
                id              TEXT PRIMARY KEY,
                type            TEXT,
                content         TEXT NOT NULL,
                tags            TEXT,       -- JSON array
                files           TEXT,       -- JSON array
                project         TEXT,
                importance      REAL DEFAULT 0.5,
                embedding       BLOB,       -- little-endian f32[]
                embedding_model TEXT,
                embedding_dims  INTEGER,
                created_at      TEXT,
                updated_at      TEXT
            );

            -- FTS5 virtual table for BM25 full-text search
            CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
                content,
                content_rowid='rowid',
                tokenize='unicode61'
            );

            -- sqlite-vec virtual table for KNN vector search
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(
                embedding float[768]
            );

            CREATE TABLE IF NOT EXISTS sessions (
                id       TEXT PRIMARY KEY,
                data     TEXT,
                metadata TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );

            CREATE TABLE IF NOT EXISTS observations (
                id         TEXT PRIMARY KEY,
                content    TEXT,
                tool_name  TEXT,
                session_id TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            );
            "
        ).context("Failed to initialize SQLite tables")?;

        Ok(())
    }

    fn needs_reindex(&self) -> Result<bool> {
        if !self.config.embed_enabled() { return Ok(false); }
        let stored: Option<(String, i32)> = self.conn
            .query_row(
                "SELECT embedding_model, embedding_dims FROM memories WHERE embedding IS NOT NULL LIMIT 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            ).ok();
        match stored {
            None => Ok(false),
            Some((model, dims)) => {
                let cfg_model = self.config.effective_model_id();
                let cfg_dims = self.config.embedding.dims as i32;
                Ok(model != cfg_model || dims != cfg_dims)
            }
        }
    }

    pub async fn reindex(&self) -> Result<usize> {
        if !self.config.embed_enabled() { return Ok(0); }
        if self.embedder.get().is_none() {
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)?;
            let _ = self.embedder.set(emb);
        }
        let mut stmt = self.conn.prepare("SELECT id, content FROM memories")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();
        if rows.is_empty() { return Ok(0); }

        let model_name = self.config.effective_model_id();
        let dims = self.config.embedding.dims as i32;
        let mut reindexed = 0usize;

        for chunk in rows.chunks(50) {
            let texts: Vec<&str> = chunk.iter().map(|(_, c)| c.as_str()).collect();
            let embeddings = match self.embedder.get() {
                Some(emb) => match emb.embed_batch(&texts).await {
                    Ok(v) => v,
                    Err(e) => { eprintln!("reindex batch error: {}", e); continue; }
                },
                None => break,
            };
            for ((id, _), vec) in chunk.iter().zip(embeddings.iter()) {
                let blob = floats_to_blob(vec);
                self.conn.execute(
                    "UPDATE memories SET embedding = ?1, embedding_model = ?2, embedding_dims = ?3 WHERE id = ?4",
                    params![blob, &model_name, dims, id],
                ).ok();
                reindexed += 1;
            }
        }
        eprintln!("Re-indexed {} memories with {} ({}d)", reindexed, model_name, dims);
        Ok(reindexed)
    }

    async fn wait_reindex(&self) {
        if self.reindexing.load(std::sync::atomic::Ordering::SeqCst) {
            let mut rx = self.reindex_ready.clone();
            if !*rx.borrow() {
                let _ = rx.changed().await;
            }
        }
    }

    // ─── CRUD ────────────────────────────────────────────────────────────────

    pub async fn save(&self, input: NewMemory) -> Result<String> {
        self.wait_reindex().await;

        let id = format!("mem_{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        let tags_json = input.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default());
        let files_json = input.files.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default());

        if self.config.embed_enabled() && self.embedder.get().is_none() {
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)?;
            let _ = self.embedder.set(emb);
        }

        let embedding_vec = match self.embedder.get() {
            Some(emb) => match emb.embed_one(input.content.as_str()).await {
                Ok(v) => Some(v),
                Err(e) => { eprintln!("embed error: {}", e); None }
            },
            None => None,
        };

        if let Some(ref vec) = embedding_vec {
            let blob = floats_to_blob(vec);
            let dims = vec.len() as i32;
            self.conn.execute(
                "INSERT INTO memories (id, type, content, tags, files, project, importance, embedding, embedding_model, embedding_dims, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
                params![id, input.memory_type, input.content, tags_json, files_json, input.project, 0.5f64, blob, self.config.effective_model_id(), dims, now],
            ).context("Failed to insert memory")?;
        } else {
            self.conn.execute(
                "INSERT INTO memories (id, type, content, tags, files, project, importance, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![id, input.memory_type, input.content, tags_json, files_json, input.project, 0.5f64, now],
            ).context("Failed to insert memory")?;
        }

        // Sync to FTS index
        let rowid: i64 = self.conn.last_insert_rowid();
        self.conn.execute(
            "INSERT INTO memories_fts(rowid, content) VALUES (?1, ?2)",
            params![rowid, input.content],
        ).ok();

        // Sync to vec0 if embedding exists
        if let Some(ref vec) = embedding_vec {
            let blob = floats_to_blob(vec);
            self.conn.execute(
                "INSERT OR REPLACE INTO vec_memories(rowid, embedding) VALUES (?1, ?2)",
                params![rowid, blob],
            ).ok();
        }

        Ok(id)
    }

    pub async fn search(&self, query: &str, limit: u8, memory_type: Option<&str>) -> Result<Vec<SearchResult>> {
        if self.config.embed_enabled() && self.embedder.get().is_none() {
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)?;
            let _ = self.embedder.set(emb);
        }
        let query_embedding = match self.embedder.get() {
            Some(emb) => match emb.embed_one(query).await {
                Ok(v) => Some(v),
                Err(e) => { eprintln!("embed error: {}", e); None }
            },
            None => None,
        };

        // FTS5 BM25 search
        let mut results: Vec<SearchResult> = Vec::new();
        let limit_i64 = limit.min(50) as i64;
        let sql = match memory_type {
            Some(t) => "SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at, m.embedding
                        FROM memories m
                        INNER JOIN memories_fts f ON m.rowid = f.rowid
                        WHERE memories_fts MATCH ?1 AND m.type = ?2
                        ORDER BY rank LIMIT ?3",
            None => "SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at, m.embedding
                      FROM memories m
                      INNER JOIN memories_fts f ON m.rowid = f.rowid
                      WHERE memories_fts MATCH ?1
                      ORDER BY rank LIMIT ?2",
        };

        let mut stmt = self.conn.prepare(sql)?;
        let rows: Vec<SearchResult> = match memory_type {
            Some(t) => stmt.query_map(params![query, t, limit_i64], |row| map_fts_row(row))?
                .filter_map(|r| r.ok()).collect(),
            None => stmt.query_map(params![query, limit_i64], |row| map_fts_row(row))?
                .filter_map(|r| r.ok()).collect(),
        };

        for mut r in rows {
                // Vector score if query embedding available and stored embedding exists
                if let (Some(qvec), Some(stored_blob)) = (&query_embedding, &r.embedding) {
                    let svec = blob_to_floats(stored_blob);
                    if svec.len() == qvec.len() {
                        r.score = cosine_similarity(qvec, &svec);
                    } else {
                        r.score = 0.5; // dimension mismatch, use BM25 score
                    }
                } else {
                    r.score = 0.5; // BM25 only
                }
            results.push(r);
        }

        // Sort by score desc, limit
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);
        Ok(results)
    }

    pub async fn list(&self, limit: u8) -> Result<Vec<Memory>> {
        let limit = limit.min(100) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, type, content, tags, files, project, importance, created_at, updated_at
             FROM memories ORDER BY created_at DESC LIMIT ?1"
        )?;
        let rows = stmt.query_map(params![limit], |row| Ok(Memory {
            id: row.get(0)?, memory_type: row.get(1)?, content: row.get(2)?,
            tags: parse_json_array(&row.get::<_, Option<String>>(3)?),
            files: parse_json_array(&row.get::<_, Option<String>>(4)?),
            project: row.get(5)?, importance: row.get(6)?,
            created_at: row.get(7)?, updated_at: row.get(8)?, session_id: None,
        }))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub async fn forget(&self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM memories WHERE id = ?1", params![id])
            .context("Failed to delete memory")?;
        Ok(())
    }

    pub async fn wipe(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM memories; DELETE FROM memories_fts; DELETE FROM sessions; DELETE FROM observations;"
        ).context("Failed to wipe database")?;
        Ok(())
    }

    pub async fn stats(&self) -> Result<DbStats> {
        Ok(DbStats {
            memory_count: self.conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0)).unwrap_or(0),
            session_count: self.conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0)).unwrap_or(0),
            observation_count: self.conn.query_row("SELECT COUNT(*) FROM observations", [], |r| r.get(0)).unwrap_or(0),
            db_path: self.path.to_string_lossy().to_string(),
        })
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

// KNN row mapper (includes score from vec0)
fn map_knn_row(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        content: row.get(2)?,
        tags: parse_json_array(&row.get::<_, Option<String>>(3)?),
        files: parse_json_array(&row.get::<_, Option<String>>(4)?),
        project: row.get(5)?,
        importance: row.get(6)?,
        score: row.get::<_, f64>(8)?,
        created_at: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        embedding: None,
    })
}

// FTS row mapper (BM25 only, no embedding)
fn map_fts_row(row: &rusqlite::Row) -> rusqlite::Result<SearchResult> {
    Ok(SearchResult {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        content: row.get(2)?,
        tags: parse_json_array(&row.get::<_, Option<String>>(3)?),
        files: parse_json_array(&row.get::<_, Option<String>>(4)?),
        project: row.get(5)?,
        importance: row.get(6)?,
        score: 0.5,
        created_at: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        embedding: None,
    })
}
fn parse_json_array(val: &Option<String>) -> Option<Vec<String>> {
    match val {
        Some(s) if !s.is_empty() => serde_json::from_str(s).ok(),
        _ => None,
    }
}

fn floats_to_blob(vec: &[f32]) -> Vec<u8> {
    let bytes: &[u8] = bytemuck::cast_slice(vec);
    bytes.to_vec()
}

fn blob_to_floats(blob: &[u8]) -> Vec<f32> {
    let (head, body, tail) = unsafe { blob.align_to::<f32>() };
    assert!(head.is_empty() && tail.is_empty(), "blob alignment error");
    body.to_vec()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { return 0.0; }
    ((dot / (na * nb)) as f64).clamp(-1.0, 1.0)
}
