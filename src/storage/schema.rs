//! Database schema initialization — tables, indexes, triggers, and vec0 virtual table.

use anyhow::Result;
use sqlx::sqlite::SqlitePool;

/// Create all tables, indexes, and triggers. Idempotent (IF NOT EXISTS).
pub(crate) async fn initialize(pool: &SqlitePool) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS _schema_version (version INTEGER PRIMARY KEY, migrated_at TEXT DEFAULT (datetime('now')))").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS memories (id TEXT PRIMARY KEY, type TEXT, content TEXT NOT NULL, tags TEXT, files TEXT, project TEXT, source_file TEXT, importance INTEGER DEFAULT 3, embedding BLOB, embedding_model TEXT, embedding_dims INTEGER, created_at TEXT, updated_at TEXT, deleted_at TEXT)").execute(pool).await?;
    let _ = sqlx::query("ALTER TABLE memories ADD COLUMN source_file TEXT")
        .execute(pool)
        .await;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_embed_null ON memories(embedding) WHERE embedding IS NULL").execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(type)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_memories_created_at ON memories(created_at)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_memories_content_type ON memories(content, type) WHERE deleted_at IS NULL")
        .execute(pool)
        .await?;
    sqlx::query("CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(content, content_rowid='rowid', tokenize='unicode61')").execute(pool).await?;
    // FTS auto-sync: INSERT trigger
    sqlx::query("CREATE TRIGGER IF NOT EXISTS memories_fts_ai AFTER INSERT ON memories WHEN new.deleted_at IS NULL BEGIN INSERT INTO memories_fts(rowid, content) VALUES (new.rowid, new.content); END;").execute(pool).await?;
    // FTS auto-sync: soft-delete removes from FTS
    sqlx::query("CREATE TRIGGER IF NOT EXISTS memories_fts_au AFTER UPDATE OF deleted_at ON memories WHEN new.deleted_at IS NOT NULL AND old.deleted_at IS NULL BEGIN INSERT INTO memories_fts(memories_fts, rowid, content) VALUES ('delete', old.rowid, old.content); END;").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS sessions (id TEXT PRIMARY KEY, data TEXT, metadata TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(pool).await?;
    sqlx::query("CREATE TABLE IF NOT EXISTS observations (id TEXT PRIMARY KEY, content TEXT, tool_name TEXT, session_id TEXT, created_at TEXT DEFAULT (datetime('now')))").execute(pool).await?;
    // Knowledge Graph triples (optional, only created if config enables it)
    let _ = sqlx::query("CREATE TABLE IF NOT EXISTS kg_triples (id TEXT PRIMARY KEY, subject TEXT NOT NULL, predicate TEXT NOT NULL, object TEXT NOT NULL, confidence REAL DEFAULT 1.0, source_memory_id TEXT, project TEXT, created_at TEXT NOT NULL)").execute(pool).await;
    let _ =
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_subject ON kg_triples(subject)")
            .execute(pool)
            .await;
    let _ =
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_object ON kg_triples(object)")
            .execute(pool)
            .await;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_kg_triples_predicate ON kg_triples(predicate)",
    )
    .execute(pool)
    .await;
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_kg_triples_memory ON kg_triples(source_memory_id)",
    )
    .execute(pool)
    .await;
    let _ =
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_kg_triples_project ON kg_triples(project)")
            .execute(pool)
            .await;
    let _ = sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_kg_triples_spo ON kg_triples(subject, predicate, object, project)").execute(pool).await;
    Ok(())
}

/// Initialize (or rebuild) the vec0 virtual table for KNN search.
pub(crate) async fn init_vec(pool: &SqlitePool, vec_dims: usize) -> Result<()> {
    // Check if existing embeddings use wrong dimensions
    // Use most common stored dims (not LIMIT 1 — might hit stale row)
    let stored_dims: Option<i64> = sqlx::query_scalar(
        "SELECT embedding_dims FROM memories WHERE embedding IS NOT NULL GROUP BY 1 ORDER BY COUNT(*) DESC LIMIT 1"
    )
    .fetch_optional(pool)
    .await?;

    let needs_rebuild =
        vec_dims > 0 && stored_dims.is_some_and(|d| d as usize != vec_dims);
    if needs_rebuild {
        log::info!(
            "init_vec: stored dims != {}, dropping vec0 + clearing embeddings",
            vec_dims
        );
        sqlx::query("DROP TABLE IF EXISTS vec_memories")
            .execute(pool)
            .await?;
        sqlx::query(
            "UPDATE memories SET embedding = NULL, embedding_model = NULL, embedding_dims = NULL"
        )
        .execute(pool)
        .await?;
    }

    sqlx::query(sqlx::AssertSqlSafe(format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS vec_memories USING vec0(embedding float[{dims}])",
        dims = vec_dims,
    )))
    .execute(pool)
    .await?;
    Ok(())
}

/// Rebuild the FTS5 index from scratch.
pub(crate) async fn rebuild_fts(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM memories_fts")
        .execute(pool)
        .await?;
    sqlx::query("INSERT INTO memories_fts(rowid, content) SELECT rowid, content FROM memories")
        .execute(pool)
        .await?;
    Ok(())
}
