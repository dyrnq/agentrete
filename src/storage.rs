//! SQLite storage via sqlx (async, Send+Sync, connection pool).

use anyhow::Result;
use chrono::Utc;
use sqlx::sqlite::SqlitePool;
use std::path::PathBuf;
use uuid::Uuid;

use crate::types::{DbStats, Memory, NewMemory, SearchResult};

#[derive(Clone)]
pub struct Store { pool: SqlitePool, path: PathBuf }

impl Store {
    pub async fn open(cfg: &crate::config::Config) -> Result<Self> {
        let path = cfg.db_dir().join("memory.db");
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let db_url = format!("sqlite:{}?mode=rwc", path.display());
        let pool = SqlitePool::connect(&db_url).await?;
        sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await?;
        sqlx::query("PRAGMA synchronous=NORMAL").execute(&pool).await?;
        sqlx::query("PRAGMA busy_timeout=5000").execute(&pool).await?;
        let store = Self { pool, path };
        store.initialize().await?;
        Ok(store)
    }

    async fn initialize(&self) -> Result<()> {
        sqlx::query("CREATE TABLE IF NOT EXISTS _schema_version (version INTEGER PRIMARY KEY, migrated_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS memories (id TEXT PRIMARY KEY, type TEXT, content TEXT NOT NULL, tags TEXT, files TEXT, project TEXT, importance REAL DEFAULT 0.5, embedding BLOB, embedding_model TEXT, embedding_dims INTEGER, created_at TEXT, updated_at TEXT)").execute(&self.pool).await?;
        sqlx::query("CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content, content_rowid='rowid', tokenize='unicode61')").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, data TEXT, metadata TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        sqlx::query("CREATE TABLE IF NOT EXISTS observations (id TEXT PRIMARY KEY, content TEXT, tool_name TEXT, session_id TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn save(&self, input: NewMemory) -> Result<String> {
        let id = format!("mem_{}", Uuid::new_v4());
        let now = Utc::now().to_rfc3339();
        let tags = input.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default());
        let files = input.files.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default());
        sqlx::query("INSERT INTO memories (id,type,content,tags,files,project,importance,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)")
            .bind(&id).bind(&input.memory_type).bind(&input.content).bind(&tags).bind(&files).bind(&input.project).bind(0.5).bind(&now)
            .execute(&self.pool).await?;
        let rowid: i64 = sqlx::query_scalar("SELECT last_insert_rowid()").fetch_one(&self.pool).await?;
        sqlx::query("INSERT INTO memories_fts(rowid,content) VALUES (?1,?2)").bind(rowid).bind(&input.content).execute(&self.pool).await?;
        Ok(id)
    }

    pub async fn search(&self, query: &str, limit: u8, memory_type: Option<&str>) -> Result<Vec<SearchResult>> {
        let lim = limit.min(100) as i64;
        let rows: Vec<SearchRow> = if let Some(t) = memory_type {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 AND m.type=?2 ORDER BY rank LIMIT ?3")
                .bind(query).bind(t).bind(lim).fetch_all(&self.pool).await?
        } else {
            sqlx::query_as("SELECT m.id, m.type, m.content, m.tags, m.files, m.project, m.importance, m.created_at FROM memories m INNER JOIN memories_fts f ON m.rowid=f.rowid WHERE memories_fts MATCH ?1 ORDER BY rank LIMIT ?2")
                .bind(query).bind(lim).fetch_all(&self.pool).await?
        };
        Ok(rows.into_iter().map(|r| SearchResult {
            id: r.id, memory_type: r.memory_type, content: r.content,
            tags: parse_json(&r.tags), files: parse_json(&r.files),
            project: r.project, importance: r.importance.unwrap_or(0.5),
            score: 0.5, created_at: r.created_at.unwrap_or_default(), embedding: None,
        }).collect())
    }

    pub async fn list(&self, limit: u8) -> Result<Vec<Memory>> {
        let rows: Vec<MemoryRow> = sqlx::query_as("SELECT id,type,content,tags,files,project,importance,created_at,updated_at FROM memories ORDER BY created_at DESC LIMIT ?1")
            .bind(limit.min(100) as i64).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| Memory {
            id: r.id, memory_type: r.memory_type, content: r.content,
            tags: parse_json(&r.tags), files: parse_json(&r.files),
            project: r.project, importance: r.importance.unwrap_or(0.5),
            created_at: r.created_at.unwrap_or_default(), updated_at: r.updated_at.unwrap_or_default(), session_id: None,
        }).collect())
    }

    pub async fn forget(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM memories WHERE id=?1").bind(id).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn wipe(&self) -> Result<()> {
        sqlx::query("DELETE FROM memories").execute(&self.pool).await?;
        sqlx::query("DELETE FROM memories_fts").execute(&self.pool).await?;
        sqlx::query("DELETE FROM sessions").execute(&self.pool).await?;
        sqlx::query("DELETE FROM observations").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn stats(&self) -> Result<DbStats> {
        let mc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories").fetch_one(&self.pool).await?;
        let sc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions").fetch_one(&self.pool).await.unwrap_or(0);
        let oc: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM observations").fetch_one(&self.pool).await.unwrap_or(0);
        Ok(DbStats { memory_count: mc, session_count: sc, observation_count: oc, db_path: self.path.to_string_lossy().to_string() })
    }
}

#[derive(sqlx::FromRow)] struct SearchRow { id: String, #[sqlx(rename="type")] memory_type: Option<String>, content: String, tags: Option<String>, files: Option<String>, project: Option<String>, importance: Option<f64>, created_at: Option<String> }
#[derive(sqlx::FromRow)] struct MemoryRow { id: String, #[sqlx(rename="type")] memory_type: Option<String>, content: String, tags: Option<String>, files: Option<String>, project: Option<String>, importance: Option<f64>, created_at: Option<String>, updated_at: Option<String> }

fn parse_json(val: &Option<String>) -> Option<Vec<String>> {
    match val { Some(s) if !s.is_empty() => serde_json::from_str(s).ok(), _ => None }
}
