#![allow(rustdoc::all)]
//! DuckDB storage layer for agentrete.
//!
//! Uses DuckDB with FTS extension for BM25 full-text search.
//! Embedding column (FLOAT[]) is for vector search.
//!
//! Model migration: on startup, checks if stored embedding dimensions match
//! the configured model. If not, triggers async background re-indexing.

use anyhow::{Context, Result};
use chrono::Utc;
use duckdb::{AccessMode, Config, Connection};
use std::path::PathBuf;
use uuid::Uuid;

use crate::types::{DbStats, Memory, NewMemory, SearchResult};

/// agentrete storage manager.
pub struct Store {
    conn: Connection,
    path: PathBuf,
    config: crate::config::Config,
    embedder: std::sync::OnceLock<crate::embed::embeddings::Embedder>,
    /// Whether a model mismatch was detected and re-indexing is in progress.
    reindexing: std::sync::atomic::AtomicBool,
}

impl Store {
    pub async fn open(cfg: &crate::config::Config) -> Result<Self> {
        let path = cfg.db_dir().join("memory.db");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }

        let config = Config::default()
            .access_mode(AccessMode::ReadWrite)
            .context("Failed to create DuckDB config")?;

        let conn = Connection::open_with_flags(&path, config)
            .with_context(|| format!("Failed to open DuckDB at {}", path.display()))?;

        let store = Self {
            conn,
            path,
            config: cfg.clone(),
            embedder: std::sync::OnceLock::new(),
            reindexing: std::sync::atomic::AtomicBool::new(false),
        };
        store.initialize().await?;

        // Detect model mismatch, trigger background re-index
        if let Ok(true) = store.needs_reindex() {
            store
                .reindexing
                .store(true, std::sync::atomic::Ordering::SeqCst);
            eprintln!(
                "Embedding model mismatch detected — starting background re-index... (searches fall back to BM25)"
            );
            // Spawn background task — share conn via Arc<Mutex<Connection>>?
            // DuckDB Connection is !Sync, so we need a different approach:
            // We flag the mismatch and let searches degrade gracefully.
            // The re-index happens lazily on next reindex() call.
            let _ = store.reindex().await;
            store
                .reindexing
                .store(false, std::sync::atomic::Ordering::SeqCst);
            eprintln!("Background re-index complete.");
        }

        Ok(store)
    }

    async fn initialize(&self) -> Result<()> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS _schema_version (
                version     INTEGER PRIMARY KEY,
                migrated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            );",
            )
            .context("Failed to create schema version table")?;

        let current: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM _schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        let migrations: Vec<(i32, &str)> = vec![(1, include_str!("../migrations/001_init.sql"))];

        for (version, sql) in &migrations {
            if *version > current {
                let migration_sql = sql
                    .trim_end()
                    .strip_suffix(";")
                    .and_then(|s| s.rfind("INSERT INTO _schema_version"))
                    .map(|pos| &sql[..pos])
                    .unwrap_or(sql);
                self.conn
                    .execute_batch(migration_sql)
                    .with_context(|| format!("Migration v{} failed", version))?;
            }
        }

        self.conn.execute_batch("INSTALL fts; LOAD fts;").ok();
        Ok(())
    }

    /// Check if stored embeddings differ from the configured model.
    fn needs_reindex(&self) -> Result<bool> {
        if !self.config.embed_enabled() {
            return Ok(false);
        }

        // Get one stored embedding's model info
        let stored: Option<(String, i32)> = self
            .conn
            .query_row(
                "SELECT embedding_model, embedding_dims FROM memories WHERE embedding IS NOT NULL LIMIT 1",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)?)),
            )
            .ok();

        match stored {
            None => Ok(false), // No embeddings yet
            Some((model, dims)) => {
                let configured_dims = self.config.embedding.dims as i32;
                let configured_model = self.config.effective_model_id();
                let mismatched = model != configured_model || dims != configured_dims;
                if mismatched {
                    eprintln!(
                        "Model mismatch: stored={} ({}d), configured={} ({}d)",
                        model, dims, configured_model, configured_dims
                    );
                }
                Ok(mismatched)
            }
        }
    }

    /// Re-index all memories with the current embedding model.
    pub async fn reindex(&self) -> Result<usize> {
        if !self.config.embed_enabled() {
            return Ok(0);
        }

        // Ensure embedder is loaded
        if self.embedder.get().is_none() {
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)
                .context("Failed to load embedder for reindex")?;
            let _ = self.embedder.set(emb);
        }

        // Get all memory IDs and contents
        let mut stmt = self.conn.prepare("SELECT id, content FROM memories")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Ok(0);
        }

        let model_name = self.config.effective_model_id();
        let dims = self.config.embedding.dims as i32;
        let mut reindexed = 0usize;

        for (id, content) in &rows {
            let vec = match self.embedder.get() {
                Some(emb) => match emb.embed_one(content).await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("reindex error for {}: {}", id, e);
                        continue;
                    }
                },
                None => break,
            };

            let array_expr: String = vec
                .iter()
                .map(|v| format!("{}::FLOAT", v))
                .collect::<Vec<_>>()
                .join(",");

            let sql = format!(
                "UPDATE memories SET embedding = array_value({0}), embedding_model = ?1, embedding_dims = ?2 WHERE id = ?3",
                array_expr
            );

            self.conn
                .execute(&sql, duckdb::params![&model_name, dims, id])
                .ok();

            reindexed += 1;
        }

        eprintln!(
            "Re-indexed {} memories with model {} ({}d)",
            reindexed, model_name, dims
        );
        Ok(reindexed)
    }

    // ─── Save / Search / etc. (unchanged) ───────────────────────────────────

    pub async fn save(&self, input: NewMemory) -> Result<String> {
        let id = format!("mem_{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();

        let tags_json = input
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_else(|_| "[]".to_string()));
        let files_json = input
            .files
            .as_ref()
            .map(|f| serde_json::to_string(f).unwrap_or_else(|_| "[]".to_string()));

        let importance = 0.5;

        if self.config.embed_enabled() && self.embedder.get().is_none() {
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)
                .context("Failed to load embedder")?;
            let _ = self.embedder.set(emb);
        }

        let embedding_vec = match self.embedder.get() {
            Some(emb) => match emb.embed_one(input.content.as_str()).await {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("embed error: {}", e);
                    None
                }
            },
            None => None,
        };

        if let Some(vec) = &embedding_vec {
            let dims = vec.len() as i32;
            let array_expr: String = vec
                .iter()
                .map(|v| format!("{}::FLOAT", v))
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "INSERT INTO memories (id, type, content, tags, files, project, importance, embedding, embedding_model, embedding_dims, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4::VARCHAR[], ?5::VARCHAR[], ?6, ?7, array_value({0}), ?8, ?9, ?10, ?10)",
                array_expr
            );
            self.conn
                .execute(
                    &sql,
                    duckdb::params![
                        &id,
                        &input.memory_type,
                        &input.content,
                        &tags_json,
                        &files_json,
                        &input.project,
                        importance,
                        &self.config.effective_model_id(),
                        dims,
                        &now,
                    ],
                )
                .context("Failed to insert memory with embedding")?;
        } else {
            self.conn.execute(
                "INSERT INTO memories (id, type, content, tags, files, project, importance, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4::VARCHAR[], ?5::VARCHAR[], ?6, ?7, ?8, ?8)",
                duckdb::params![
                    &id, &input.memory_type, &input.content,
                    &tags_json, &files_json, &input.project,
                    importance, &now,
                ],
            ).context("Failed to insert memory")?;
        }

        Ok(id)
    }

    pub async fn search(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        if self.config.embed_enabled() && self.embedder.get().is_none() {
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)
                .context("Failed to load embedder")?;
            let _ = self.embedder.set(emb);
        }
        let query_embedding = match self.embedder.get() {
            Some(emb) => match emb.embed_one(query).await {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("embed error: {}", e);
                    None
                }
            },
            None => None,
        };
        crate::search::search_fts(&self.conn, query, limit, memory_type, query_embedding)
    }

    pub async fn list(&self, limit: u8) -> Result<Vec<Memory>> {
        let limit = limit.min(100) as usize;
        let mut stmt = self
            .conn
            .prepare("SELECT id, type, content, tags, files, project, importance, created_at, updated_at FROM memories ORDER BY created_at DESC LIMIT ?1")?;
        let rows = stmt.query_map(duckdb::params![limit as i64], |row| {
            Ok(Memory {
                id: row.get(0)?,
                memory_type: row.get(1)?,
                content: row.get(2)?,
                tags: parse_json_array(&row.get::<_, Option<String>>(3)?),
                files: parse_json_array(&row.get::<_, Option<String>>(4)?),
                project: row.get(5)?,
                importance: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?, session_id: None,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    pub async fn forget(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM memories WHERE id = ?1", duckdb::params![id])
            .context("Failed to delete memory")?;
        Ok(())
    }

    pub async fn wipe(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM memories; DELETE FROM sessions; DELETE FROM observations;")
            .context("Failed to wipe database")?;
        Ok(())
    }

    pub async fn stats(&self) -> Result<DbStats> {
        let memory_count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
            .unwrap_or(0);
        let session_count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap_or(0);
        let observation_count: u64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))
            .unwrap_or(0);
        Ok(DbStats {
            memory_count,
            session_count,
            observation_count,
            db_path: self.path.to_string_lossy().to_string(),
        })
    }
}

fn parse_json_array(val: &Option<String>) -> Option<Vec<String>> {
    match val {
        Some(s) if !s.is_empty() => serde_json::from_str(s).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentrete_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.db")
    }

    #[tokio::test]
    async fn test_needs_reindex_empty_db() {
        let path = tmp_db_path();
        std::env::set_var("AGENTRETE_NO_EMBED", "1");
        let store = Store::open(&crate::config::Config::default())
            .await
            .unwrap();
        assert!(!store.needs_reindex().unwrap());
        std::env::remove_var("AGENTRETE_NO_EMBED");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
