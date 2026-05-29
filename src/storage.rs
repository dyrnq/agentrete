#![allow(rustdoc::all)]
//! DuckDB storage layer for agentrete.
//!
//! Uses DuckDB with FTS extension for BM25 full-text search.
//! Embedding column (FLOAT[1024]) is pre-created for future vector search.

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
}

impl Store {
    /// Open or create the database.
    pub async fn open(cfg: &crate::config::Config) -> Result<Self> {
        let path = cfg.db_dir().join("memory.db");

        // Ensure parent directory exists
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
        };
        store.initialize().await?;

        Ok(store)
    }

    /// Initialize tables and run pending migrations.
    ///
    /// Migration strategy:
    /// - Only ADD operations (create table, add column, add index)
    /// - Never DROP operations (drop table, drop column, drop index)
    /// - This ensures binary rollback is always safe:
    ///   v0.0.1 created DB, v0.0.2 added a column, rolled back to v0.0.1
    ///   → old binary reads new DB safely (extra columns are ignored)
    /// - To drop something, wait until the next major version and document
    ///   the breaking change in release notes.
    async fn initialize(&self) -> Result<()> {
        // Ensure schema version table exists
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

        // Embedded migrations (compiled into binary via include_str!)
        let migrations: Vec<(i32, &str)> = vec![(1, include_str!("../migrations/001_init.sql"))];

        for (version, sql) in &migrations {
            if *version > current {
                // Strip the trailing INSERT INTO _schema_version (initialize() manages versions)
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

        // Load FTS extension
        self.conn.execute_batch("INSTALL fts; LOAD fts;").ok();

        Ok(())
    }

    /// Save a new memory.
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

        // Lazy init: load embedding model on first save (unless disabled)
        if self.config.embed_enabled() && self.embedder.get().is_none() {
            eprintln!(
                "Loading embedding model (backend={:?}, model={})...",
                self.config.embedding.backend, self.config.embedding.model_id
            );
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)
                .context("Failed to load embedder")?;
            let _ = self.embedder.set(emb);
        }

        let embedding_vec = match self.embedder.get() {
            Some(emb) => match emb.embed_one(input.content.as_str()).await {
                Ok(v) => Some(v),
                Err(e) => { eprintln!("embed error: {}", e); None }
            },
            None => None,
        };

        if let Some(vec) = &embedding_vec {
            let dims = vec.len() as i32;
            // Build array_value inline: array_value(v1::FLOAT, v2::FLOAT, ...)
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
                        "m3e-base",
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

    /// Search memories using FTS (BM25).
    pub async fn search(
        &self,
        query: &str,
        limit: u8,
        memory_type: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        // Lazy load embedding model for vector search
        if self.config.embed_enabled() && self.embedder.get().is_none() {
            eprintln!(
                "Loading embedding model for search (backend={:?})...",
                self.config.embedding.backend
            );
            let emb = crate::embed::embeddings::Embedder::from_config(&self.config.embedding)
                .context("Failed to load embedder")?;
            let _ = self.embedder.set(emb);
        }
        let query_embedding = match self.embedder.get() {
            Some(emb) => match emb.embed_one(query).await {
                Ok(v) => Some(v),
                Err(e) => { eprintln!("embed error: {}", e); None }
            },
            None => None,
        };
        crate::search::search_fts(&self.conn, query, limit, memory_type, query_embedding)
    }

    /// List recent memories.
    pub async fn list(&self, limit: u8) -> Result<Vec<Memory>> {
        let limit = limit.min(100) as i64;
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, type, content, tags::VARCHAR, files::VARCHAR,
                    project, importance, created_at::VARCHAR as created_at, updated_at::VARCHAR as updated_at
             FROM memories
             ORDER BY created_at DESC
             LIMIT ?1"
        )?;

        let rows = stmt.query_map(duckdb::params![limit], |row| {
            Ok(Memory {
                id: row.get(0)?,
                session_id: row.get(1)?,
                memory_type: row.get(2)?,
                content: row.get(3)?,
                tags: parse_json_array(&row.get::<_, Option<String>>(4)?),
                files: parse_json_array(&row.get::<_, Option<String>>(5)?),
                project: row.get(6)?,
                importance: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Delete a memory by ID.
    pub async fn forget(&self, id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM memories WHERE id = ?1", duckdb::params![id])
            .context("Failed to delete memory")?;
        Ok(())
    }

    /// Delete all memories.
    pub async fn wipe(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM memories; DELETE FROM sessions; DELETE FROM observations;")
            .context("Failed to wipe database")?;
        Ok(())
    }

    /// Get database statistics.
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

/// Parse a JSON array string into Vec<String>.
fn parse_json_array(val: &Option<String>) -> Option<Vec<String>> {
    match val {
        Some(s) if !s.is_empty() => serde_json::from_str(s).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_db_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agentrete_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("test.db")
    }

    #[allow(dead_code)]
    /// Override db_path to use temp dir for tests
    fn with_test_store<F>(f: F)
    where
        F: std::future::Future<Output = ()>,
    {
        let path = tmp_db_path();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            std::env::set_var("DATA_DIR", path.parent().unwrap().to_str().unwrap());
            let store = Store::open(&crate::config::Config::default())
                .await
                .unwrap();
            store.wipe().await.unwrap();
            f.await;
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        });
    }

    #[tokio::test]
    async fn test_embed_disabled_from_config() {
        std::env::set_var("AGENTRETE_NO_EMBED", "1");
        let cfg = crate::config::Config::default();
        assert!(cfg.embed_enabled()); // disabled by env var
        std::env::remove_var("AGENTRETE_NO_EMBED");
        let cfg = crate::config::Config::default();
        assert!(cfg.embed_enabled());
    }

    #[tokio::test]
    async fn test_embed_disabled_false() {
        std::env::set_var("AGENTRETE_NO_EMBED", "0");
        let cfg = crate::config::Config::default();
        assert!(cfg.embed_enabled());
        std::env::remove_var("AGENTRETE_NO_EMBED");
    }

    #[tokio::test]
    async fn test_store_open_no_embed() {
        std::env::set_var("AGENTRETE_NO_EMBED", "1");
        let path = tmp_db_path();
        std::env::set_var("DATA_DIR", path.parent().unwrap().to_str().unwrap());

        let store = Store::open(&crate::config::Config::default())
            .await
            .unwrap();
        assert!(store.embedder.get().is_none());
        assert!(store.embedder.get().is_none());

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.memory_count, 0);

        std::env::remove_var("AGENTRETE_NO_EMBED");
        std::env::remove_var("DATA_DIR");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn test_save_and_search() {
        std::env::set_var("AGENTRETE_NO_EMBED", "1");
        let path = tmp_db_path();
        std::env::set_var("DATA_DIR", path.parent().unwrap().to_str().unwrap());

        let store = Store::open(&crate::config::Config::default())
            .await
            .unwrap();
        let id = store
            .save(NewMemory {
                content: "单元测试保存".to_string(),
                memory_type: Some("test".to_string()),
                tags: Some(vec!["rust".to_string(), "test".to_string()]),
                files: None,
                project: Some("agentrete".to_string()),
            })
            .await
            .unwrap();
        assert!(id.starts_with("mem_"));

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.memory_count, 1);

        std::env::remove_var("AGENTRETE_NO_EMBED");
        std::env::remove_var("DATA_DIR");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }

    #[tokio::test]
    async fn test_store_open_without_flag() {
        // Default (no env var) - embedder should be empty but ready for lazy init
        let path = tmp_db_path();
        std::env::set_var("DATA_DIR", path.parent().unwrap().to_str().unwrap());

        let store = Store::open(&crate::config::Config::default())
            .await
            .unwrap();
        // OnceLock is empty (lazy init), not disabled
        assert!(store.embedder.get().is_none());

        std::env::remove_var("DATA_DIR");
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
