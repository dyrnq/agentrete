# Agentrete Memory Lifecycle

How memories flow from creation to retrieval.

## Architecture Overview

```
┌─────────────────────────────────────────────┐
│  Codex CLI    │    CLI tools                │
│  (stdio)      │    (agentrete save/search)  │
└──────┬──────────────────┬───────────────────┘
       │                  │
       ▼                  ▼
┌──────────────┐  ┌──────────────────────┐
│  MCP stdio   │  │  MCP HTTP (axum)     │
│  (line-JSON) │  │  (POST /)            │
└──────┬───────┘  └──────────┬───────────┘
       │                     │
       └──────────┬──────────┘
                  ▼
        ┌─────────────────┐
        │  Storage Layer  │
        │  (sqlx + SQLite)│
        └────────┬────────┘
                 │
    ┌────────────┼────────────┐
    ▼            ▼            ▼
┌────────┐ ┌──────────┐ ┌──────────────┐
│ SQLite │ │ FTS5     │ │ Embed Worker │
│ (WAL)  │ │ (BM25)   │ │ (HTTP only)  │
└────────┘ └──────────┘ └──────────────┘
```

## Memory Storage

Each memory is stored in SQLite with structure:

```sql
CREATE TABLE memories (
    id              TEXT PRIMARY KEY,
    type            TEXT,           -- rule, decision, pattern, bug, fact
    content         TEXT NOT NULL,
    tags            TEXT,           -- JSON array
    files           TEXT,           -- JSON array
    project         TEXT,
    importance      REAL DEFAULT 0.5,
    embedding       BLOB,           -- model_name + dims stored separately
    embedding_model TEXT,
    embedding_dims  INTEGER,
    created_at      TEXT,
    updated_at      TEXT
);

CREATE VIRTUAL TABLE memories_fts USING fts5(content, ...);
```

## Save Flow

```
User/Codex says "remember: xxx"
        │
        ▼
  memory_save(content, type, tags)
        │
        ▼
  Store::save()
    ├─ INSERT INTO memories (embedding = NULL)
    ├─ INSERT INTO memories_fts (rowid, content)
    └─ Return mem_{uuid}
        │
        ▼
  [HTTP mode only: embed worker]
    ├─ SELECT WHERE embedding IS NULL LIMIT 500
    ├─ Ollama /api/embed batch
    └─ UPDATE memories SET embedding = ?
```

**Key design**: Save never blocks on embedding. Embedding is computed asynchronously by a background worker in HTTP mode. stdio mode skips embedding entirely — use HTTP mode for embeddings.

## Search Flow

```
User/Codex searches memories
        │
        ▼
  memory_search(query, limit)
        │
        ▼
  Store::search()
    ├─ FTS5 MATCH query → BM25 keyword ranking
    └─ Return top N with scores
```

Search is FTS5 BM25 keyword matching. Fast, works offline, no API call needed.

## Model Switching

When the embedding model changes (e.g., qwen3 → granite), the embed worker automatically recomputes all vectors:

```sql
SELECT id, content FROM memories
WHERE embedding IS NULL
   OR embedding_model IS NOT ?   -- old model name
   OR embedding_dims IS NOT ?    -- old dimensions
ORDER BY embedding IS NULL DESC
```

This ensures a clean transition with no stale vectors.

## Transports

| Transport | Embedding | Use Case |
|-----------|-----------|----------|
| **HTTP** (axum, port 9092) | ✅ embed worker runs | Production, shared by all Codex instances |
| **stdio** (line-JSON) | ❌ no embed worker | Per-instance, fast CRUD only |

## Files on Disk

```
~/.agentrete/
├── config.toml                        ← user configuration
├── config.yaml                        ← alternative format
└── memory.db                          ← SQLite database + FTS5 index

~/.cache/huggingface/hub/
└── models--BAAI--bge-small-zh-v1.5/  ← local model cache
```

## Performance

| Operation | Throughput | Notes |
|-----------|-----------|-------|
| Save (HTTP) | 155 req/s | embedding deferred |
| Save (stdio) | 139 req/s | pure SQLite + FTS5 |
| Search (HTTP) | 37,700 req/s | FTS5 BM25, concurrent |
| Search (stdio) | 264 req/s | serial, line-by-line |
| Embed worker | ~100 vectors/s | Ollama batch 500, LAN |

## Tech Stack

| Component | Technology |
|-----------|-----------|
| Database | SQLite (WAL mode, sqlx) |
| Full-text search | FTS5 (BM25) |
| Vector search | deferred (embed worker + BLOB storage) |
| Sessions & Observations | Auto-recorded on MCP init / memory_save / memory_search |
| Embedding model | minilm-256d (local, 256d, 131MB) |
| Embedding remote | qwen3-embedding (Ollama, 4096d) |
| HTTP framework | axum |
| Config | config-rs (TOML/YAML/JSON + env) |
