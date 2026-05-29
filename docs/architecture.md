# Agentrete System Architecture

> **Search Engine**: sqlite-vec KNN (primary) → FTS5 cosine rerank → FTS5 BM25

## Overview

Agentrete is a local-first persistent memory engine for AI coding agents. It exposes MCP tools over HTTP/stdio, stores context in a single SQLite file with sqlite-vec KNN search + FTS5 full-text fallback, and optionally computes embedding vectors via remote API (Ollama / OpenAI / Anthropic) in a background worker.


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
        ┌─────────────┼──────────────┐
        ▼             ▼              ▼
  ┌──────────┐ ┌───────────┐ ┌───────────┐
  │ SQLite   │ │ sqlite-vec│ │ Embed     │
  │ (sqlx)   │ │  (KNN)    │ │ Worker    │
  └──────────┘ └───────────┘ └───────────┘
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
├── config.rs            Config loading (TOML/YAML/JSON + env), EmbeddingConfig
├── mcp/
│   ├── mod.rs           Module declarations, re-exports
│   ├── handlers.rs      RPC dispatch, tools, version negotiation
│   ├── transport_http.rs axum Streamable HTTP server
│   ├── transport_stdio.rs stdio JSON-RPC transport
│   ├── v2024.rs         2024-11-05 protocol (HTTP+SSE)
│   ├── v2025_06.rs      2025-06-18 protocol (Streamable HTTP)
│   └── v2025_11.rs      2025-11-25 protocol (Stable)
├── storage.rs           SQLite via sqlx, sqlite-vec KNN, FTS5 fallback, embed_pending()
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
    ├── pre-tool-use.ps1
    ├── post-tool-use.ps1
    ├── pre-compact.ps1
    ├── post-compact.ps1
    ├── subagent-start.ps1
    ├── subagent-stop.ps1
    └── stop.ps1

ext/
├── vec0-linux-x86_64.so      sqlite-vec extension (embedded at compile time)
└── vec0.so                   Generic fallback copy
```

## Search Architecture

Memory search uses a **3-tier auto-selecting dispatch**:

```
search(query)
    │
    ├─ embedder available + vec_enabled?
    │   YES → vec0 KNN (cosine from L2)
    │          ├─ hit  → return results
    │          └─ miss → fall through
    │
    ├─ embedder available?
    │   YES → FTS5 recall + cosine rerank (hybrid)
    │          └─ return results
    │
    └─ FALLBACK → FTS5 BM25 keyword only
```

### Tier 1: sqlite-vec KNN

- **Extension**: `sqlite-vec` v0.1.10-alpha.4, statically embedded via `include_bytes!()`
- **Loading**: Extracted to system temp dir at startup, loaded via `SqliteConnectOptions::extension_with_entrypoint("sqlite3_vec_init")` on sqlx 0.9 with `sqlite-load-extension` feature
- **Query flow**: embed query → normalize to L2 unit vector → `vec_memories MATCH ?1 AND k = ?2` → score = `max(0, 1 - L2²/2)` (cosine-equivalent)
- **Data flow**: embed worker writes `vec_memories` rows alongside `memories.embedding` BLOB

### Tier 2: Hybrid FTS5 + Cosine Rerank

- FTS5 BM25 recall → embed query → cosine similarity on top-N candidate embeddings → return sorted

### Tier 3: FTS5 BM25 Only

- `unicode61` tokenizer, single pass `ORDER BY rank`

### Current Status

sqlite-vec KNN is **active and working**. Verified on Debian 12 x86_64 with 11000+ memories:

```
search: vec0 KNN hit (5 results, top score=0.774)
```

## Transports

| Transport | Embed Worker | Use Case | Details |
|-----------|-------------|----------|---------|
| **Streamable HTTP** | ✅ | Production, shared | axum on 127.0.0.1:{port}, POST `/` for JSON-RPC, GET `/` for health |
| **stdio** | ❌ | Per-instance, CRUD only | stdin/stdout line-delimited JSON-RPC, no embedding computation |

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
| **Local** (candle) | `backend = "local"` | 512d (bge-small) | Sequential | — |
| **Remote Ollama** | `backend = "remote"`, `vendor = "ollama"` | 768/4096 | ✅ native batch | None |
| **Remote OpenAI** | `backend = "remote"`, `vendor = "openai"` | model-dependent | ✅ native batch | API key |
| **Remote Anthropic** | `backend = "remote"`, `vendor = "anthropic"` | model-dependent | ✅ native batch | API key |

**Remote vendor auto-detection**: If `vendor` is not explicitly set, the URL is inspected:
- Contains `:11434` or `ollama` → Ollama
- Contains `anthropic` → Anthropic
- Otherwise → OpenAI

## Performance

Benchmarks on 8-CPU Debian 12, Ollama (`qwen3-embedding:latest`, 4096d) on LAN:

| Operation | Throughput | Notes |
|-----------|-----------|-------|
| Save (HTTP) | **~4,700 req/s** | 20 concurrent, embedding deferred to worker |
| Embed worker digest | **~56 vectors/s** | Batch 500, ~9s per round (remote Ollama LAN) |
| Search (vec0 KNN) | **~5 req/s** | Per-search: embed query + KNN + score calc |
| Search (FTS5 only) | **~37,700 req/s** | 100 concurrent, BM25 keyword only |

Key design: **save never waits for embedding**. Embedding is computed asynchronously by a background worker polling for `embedding IS NULL` rows, calling the remote API in batches of 500. Model change triggers automatic recompute (`WHERE embedding_model IS NOT ?`).

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
| `sqlx` (sqlite, sqlite-load-extension) | 0.9 | Async SQLite with connection pool + extension loading |
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

1. **SQLite + sqlx 0.9 with sqlite-vec**: Pure Rust async, `sqlite-load-extension` feature enabled, `extension_with_entrypoint("sqlite3_vec_init")` at pool creation time. Extension `.so` embedded via `include_bytes!()` and extracted to temp dir at runtime.
2. **sqlite-vec KNN as primary search**: L2-normalized query embedding → vec0 virtual table MATCH → cosine-equivalent scoring. Falls back to FTS5 on error/empty.
3. **Embed worker, not inline**: Save never blocks on embedding API call; background poll-loop calls remote Ollama/OpenAI/Anthropic in batched mode.
4. **Partial index on NULL**: `CREATE INDEX ... WHERE embedding IS NULL` — only pending rows indexed for efficient worker polling.
5. **Model change = automatic recompute**: `WHERE embedding IS NULL OR embedding_model IS NOT ?` catches old vectors when model or dimension changes.
6. **axum + sqlx Send+Sync stack**: Full async, connection pool, WAL mode, no Mutex needed.
7. **Remote vendor pluggable**: OpenAI / Anthropic / Ollama each in own module, auto-detected from URL.
8. **Version-negotiated MCP**: Clean separation per protocol version (2024-11-05, 2025-06-18, 2025-11-25), easy to add future versions.
9. **Hooks embedded at compile time**: All scripts via `include_str!()`, no external files needed at deploy time.
10. **jemalloc**: Faster memory allocation for long-running server processes.

## Model2Vec Backend (NEW)

Model2Vec is a static embedding approach that distills sentence-transformers into compact,
ultra-fast lookup tables. No GPU needed — runs entirely on CPU at ~0.1ms per text.

| Property | Value |
|----------|-------|
| Model size | **10MB** (vs 93MB for candle BERT) |
| Load time | **0.9s** |
| Encode speed | **0.17ms/text** (1000x faster than candle, 100x faster than Ollama) |
| Dimension | 256d (configurable, depends on source model) |
| Embedding | **Inline** (computed during save, no worker needed) |
| vec0 KNN | ✅ Supported |

**How it works**: A sentence-transformers model (e.g. `BAAI/bge-small-zh-v1.5`) is distilled
into static token embeddings via `model2vec.distill()`. The resulting files
(tokenizer.json + model.safetensors + config.json) are loaded by `model2vec-rs`.
Encoding is pure weighted token averaging — no neural network forward pass.

**Configuration**:
```toml
[embedding]
backend = "model2vec"

[embedding.local]
model = "BAAI/bge-small-zh-v1.5"
dims = 256
model2vec_path = "/path/to/distilled/model"
```

**Distillation** (one-time):
```bash
pip install model2vec[distill]
python3 -c "
from model2vec.distill import distill
m = distill(model_name='BAAI/bge-small-zh-v1.5')
m.save_pretrained('./model2vec-bge-small-zh')
"
```

**Trade-offs**:
- ✅ Fastest embedding backend available
- ✅ No GPU, no network, no external service
- ✅ Tiny model (10MB), can be embedded in binary
- ⚠️ Lower semantic accuracy than 4096d qwen3 (~0.73 vs ~0.84)
- ⚠️ Requires one-time distillation (30s CPU)
