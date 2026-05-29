# Agentrete System Architecture

## Overview

Agentrete is a local-first persistent memory engine for AI coding agents. It exposes MCP tools over HTTP/stdio, stores context in a single SQLite file with FTS5 full-text search, and optionally computes embedding vectors via remote API (Ollama / OpenAI / Anthropic) in a background worker.

```
┌──────────────────────────────────────────────────────────┐
│  Codex CLI  │  CLI (agentrete save/search)  │  REST API │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
              ┌────────────────┐
              │   MCP Layer    │
              │  (mcp/handlers)│
              └───────┬────────┘
                      │
          ┌───────────┼───────────┐
          ▼           ▼           ▼
    ┌──────────┐ ┌──────────┐ ┌──────────┐
    │ v2024    │ │ v2025_06 │ │ v2025_11 │
    │ (SSE)    │ │ (Stream) │ │ (Stable) │
    └──────────┘ └──────────┘ └──────────┘
                      │
                      ▼
              ┌────────────────┐
              │  Storage Layer │
              │ (storage.rs)   │
              └───────┬────────┘
                      │
          ┌───────────┼───────────┐
          ▼           ▼           ▼
    ┌──────────┐ ┌──────────┐ ┌──────────┐
    │ SQLite   │ │ FTS5     │ │ Embed    │
    │ (sqlx)   │ │ (BM25)   │ │ Worker   │
    └──────────┘ └──────────┘ └──────────┘
```

## Source Tree

```
src/
├── main.rs              Entry point, CLI dispatch, embed worker spawn
├── cli/
│   ├── mod.rs
│   ├── setup_wizard.rs  Auto-detect AI tools, configure MCP + hooks
│   ├── hooks.rs         Hook script installer (embedded at compile time)
│   └── daemon.rs        Cross-platform background service management
├── config.rs            Config loading (TOML/YAML/JSON + env), RemoteVendor, EmbeddingConfig
├── mcp/
│   ├── mod.rs           Module declarations, re-exports
│   ├── handlers.rs      RPC dispatch, tools, version negotiation
│   ├── transport_http.rs axum Streamable HTTP server
│   ├── transport_stdio.rs stdio JSON-RPC transport
│   ├── v2024.rs         2024-11-05 protocol (HTTP+SSE)
│   ├── v2025_06.rs      2025-06-18 protocol (Streamable HTTP)
│   └── v2025_11.rs      2025-11-25 protocol (Stable)
├── storage.rs           SQLite via sqlx, FTS5, embed_pending(), partial index
├── embed/
│   ├── mod.rs           candle BERT model loader (local backend)
│   ├── models.rs        Model presets constants
│   ├── embeddings.rs    Embedder enum (Local / OpenAI / Anthropic / Ollama)
│   └── remote/
│       ├── mod.rs       RemoteEmbedder enum + RemoteProvider::detect()
│       ├── openai.rs    OpenAI-compatible embeddings endpoint
│       ├── anthropic.rs Anthropic embeddings endpoint
│       └── ollama.rs    Ollama embeddings endpoint
└── types.rs             Data structures (Memory, NewMemory, SearchResult, DbStats)

hooks/
├── unix/
│   ├── _json_extract.sh      Shared JSON helpers (python3 → jq fallback)
│   ├── hooks.codex.json      Codex hook manifest
│   ├── session-start.sh      Load project context on session start
│   ├── prompt-submit.sh      Search memories on user prompt
│   ├── pre-tool-use.sh       Pre-write hook
│   ├── post-tool-use.sh      Auto-save write/exec operations
│   ├── pre-compact.sh        Snapshot context before compaction
│   ├── post-compact.sh       Reload memories after compaction
│   ├── subagent-start.sh     Load rules for subagents
│   ├── subagent-stop.sh      Save subagent completion
│   ├── stop.sh               Session end
│   ├── claude-startup.sh     Claude Code session-start hook
│   └── claude-post-tool.sh   Claude Code post-tool-use hook
└── windows/
    ├── hooks.codex.json      Codex hook manifest (PowerShell)
    ├── session-start.ps1
    ├── prompt-submit.ps1
    ├── ...                   (mirror of unix/ scripts in PowerShell)
    └── claude-post-tool.ps1
```

## Data Flow

### Memory Save (without embedding — fast path)

```
User/Codex "remember: xxx"
        │
        ▼
  memory_save(content, type, tags)
        │
        ▼
  Store::save()
    ├─ SQLite INSERT INTO memories (embedding = NULL)
    ├─ FTS5 INSERT for full-text index
    └─ Return mem_{uuid}
```

### Memory Save with Embedding Worker

```
Store::save()
  │
  ├─ INSERT (embedding = NULL)  ← fast, no embedding wait
  │
  ▼
[Background: embed worker loop]
  │   SELECT id, content FROM memories
  │   WHERE embedding IS NULL OR embedding_model != ? OR embedding_dims != ?
  │   ORDER BY embedding IS NULL DESC, created_at ASC
  │   LIMIT 500
  │
  ├─ Ollama /api/embed batch (500 inputs → 500 vectors)
  │
  └─ UPDATE memories SET embedding=?, embedding_model=?, embedding_dims=?
      (per-row, within a single batch)
```

### Embed Worker Behavior

| Condition | Action |
|-----------|--------|
| `embedding IS NULL` rows exist | Query 500, batch embed, UPDATE |
| No pending rows | Sleep 5s, retry |
| Embed API error | Sleep 10s, retry with same batch |
| Model changed in config | `embedding_model != ?` catches old rows → full recompute |
| Dimension mismatch | `embedding_dims != ?` catches stale rows |

### Memory Search

```
User/Codex "search memories for: xxx"
        │
        ▼
  memory_search(query, limit)
        │
        ▼
  Store::search()
    ├─ FTS5 MATCH query → BM25 keyword ranking
    └─ Return top N with scores
```

## Transports

| Transport | Use Case | Details |
|-----------|----------|---------|
| **Streamable HTTP** | Codex MCP HTTP mode | axum on 127.0.0.1:{port}, POST `/` for JSON-RPC, GET `/` for health |
| **stdio** | Codex MCP stdio mode | stdin/stdout line-delimited JSON-RPC |

## MCP Protocol Compliance

Supports three protocol versions with version-specific initialize handlers:

| Version | Transport | Capabilities |
|---------|-----------|-------------|
| 2024-11-05 | HTTP+SSE | tools (SSE streaming not implemented) |
| 2025-06-18 | Streamable HTTP | tools |
| 2025-11-25 | Streamable HTTP | tools, ping, version negotiation |

Version negotiation: client sends `protocolVersion` in `initialize` → server matches against supported list → returns matching version or `-32602` error.

## Embedding Backends

| Backend | Config | Dimension | Batch | Auth |
|---------|--------|-----------|-------|------|
| **None** | `backend = "none"` | — | — | — |
| **Local** (candle) | `backend = "local"` | model-dependent | Sequential | — |
| **Remote Ollama** | `backend = "remote"`, `remote_vendor = "ollama"` | 768/4096 | ✅ native batch | None |
| **Remote OpenAI** | `backend = "remote"`, `remote_vendor = "openai"` | model-dependent | ✅ native batch | API key |
| **Remote Anthropic** | `backend = "remote"`, `remote_vendor = "anthropic"` | model-dependent | ✅ native batch | API key |

**Remote vendor auto-detection**: If `remote_vendor` is not explicitly set, the URL is inspected:
- Contains `:11434` or `ollama` → Ollama
- Contains `anthropic` → Anthropic
- Otherwise → OpenAI

## Performance

Benchmarks on 8-CPU Debian 12, Ollama (`qwen3-embedding:latest`, 4096d) on LAN:

| Operation | Throughput | Notes |
|-----------|-----------|-------|
| Save (HTTP) | **155-162 req/s** | 200 concurrent, embedding deferred to worker |
| Embed worker digest | **~100 vectors/s** | Batch 500, ~5s per round |
| Search (HTTP) | **37,700 req/s** | 100 concurrent, FTS5 only |

Key design: **save never waits for embedding**. Embedding is computed asynchronously by a background worker polling for `embedding IS NULL` rows, calling the remote API in batches of 500.

## Hooks Integration

### Supported Agents

| Agent | MCP Config | Hooks |
|-------|-----------|-------|
| **Codex CLI** | TOML (config.toml) | 9 events (bash on Unix, PowerShell on Windows) |
| **Claude Code** | JSON (settings.json) | 2 events: SessionStart, PostToolUse |
| **Cursor** | JSON (mcp.json) | MCP tools only |
| **Zed** | JSON (settings.json) | MCP tools only |
| **OpenCode** | JSON (opencode.json) | MCP tools only |
| **Windsurf** | JSON (mcp_config.json) | MCP tools only |
| **Goose** | YAML (config.yaml) | MCP tools only |
| **Gemini CLI** | JSON (settings.json) | MCP tools only |

### JSON Helper Fallback

Unix hooks use `_json_extract.sh` with automatic fallback:

```
python3  →  jq  →  empty default
 (if avail) (if avail) (always safe)
```

No hooks fail due to missing runtime dependencies.

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `sqlx` (sqlite) | 0.8 | Async SQLite with connection pool |
| `axum` | 0.8 | HTTP server (Streamable HTTP transport) |
| `candle-core` / `candle-transformers` / `candle-nn` | 0.10 | On-device BERT embedding (local backend) |
| `tokenizers` | 0.19 | HuggingFace tokenizer |
| `hf-hub` | 0.5 | HuggingFace model download |
| `reqwest` | 0.12 | HTTP client (install-model, remote embed) |
| `tikv-jemallocator` | 0.7 | jemalloc global allocator |
| `clap` | 4.x | CLI argument parsing |
| `uuid` | 1.x | Memory ID generation |
| `serde` / `serde_json` | 1.x | JSON + config serialization |
| `tokio` | 1.x | Async runtime |

## Key Design Decisions

1. **SQLite + sqlx over DuckDB**: Pure Rust async, no `!Sync` issues, axum compatible, simpler deployment
2. **FTS5 over vector search as primary**: BM25 keyword match is fast, sufficient for structured memory; embedding vectors are computed asynchronously as secondary signal
3. **Embed worker, not inline**: Save never blocks on embedding API call; background poll-loop batched Ollama 500 at a time
4. **Partial index on NULL**: `CREATE INDEX ... WHERE embedding IS NULL` — only pending rows indexed
5. **Model change = automatic recompute**: `WHERE embedding IS NULL OR embedding_model IS NOT ?` catches old vectors
6. **axum + sqlx Send+Sync stack**: Full async, connection pool, WAL mode, no Mutex needed
7. **Remote vendor pluggable**: OpenAI / Anthropic / Ollama each in own module, auto-detected from URL
8. **Version-negotiated MCP**: Clean separation per protocol version, easy to add future versions
9. **Hooks embedded at compile time**: All scripts via `include_str!()`, no external files needed at deploy time
10. **jemalloc**: Faster memory allocation for long-running server processes
