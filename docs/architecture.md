# Agentrete System Architecture

## Overview

Agentrete is a local-first persistent memory engine for AI coding agents. It exposes MCP tools over HTTP/stdio, stores context in a single DuckDB file, and uses an on-device embedding model (m3e-base) for semantic search.

```
┌──────────────────────────────────────────────────────────┐
│  Codex CLI  │  CLI (agentrete save/search)  │  REST API │
└──────────────────────┬───────────────────────────────────┘
                       │
                       ▼
              ┌────────────────┐
              │   MCP Layer    │
              │  (mcp/mod.rs)  │
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
    │ DuckDB   │ │ FTS      │ │ Embed    │
    │ (OLAP)   │ │ (BM25)   │ │ (m3e)    │
    └──────────┘ └──────────┘ └──────────┘
```

## Source Tree

```
src/
├── main.rs              Entry point, CLI dispatch
├── cli/
│   ├── mod.rs
│   ├── setup_wizard.rs  Auto-detect AI tools, configure MCP + hooks
│   ├── hooks.rs         Hook script installer (embedded at compile time)
│   └── daemon.rs        Cross-platform background service management
├── mcp/
│   ├── mod.rs           Transport (stdio/HTTP), RPC dispatch, tools
│   ├── v2024.rs         2024-11-05 protocol (HTTP+SSE)
│   ├── v2025_06.rs      2025-06-18 protocol (Streamable HTTP)
│   └── v2025_11.rs      2025-11-25 protocol (Stable)
├── storage.rs           DuckDB storage, migrations, embedding write/read
├── search.rs            BM25 FTS + vector similarity hybrid search
├── embed/
│   ├── mod.rs           candle BERT model loader, embedding compute
│   └── models.rs        Model presets (m3e-base, bge-small, etc.)
└── types.rs             Data structures (Memory, NewMemory, SearchResult, DbStats)

hooks/
├── unix/
│   ├── hooks.codex.json         Codex hook manifest (bash, ${HOME} paths)
│   ├── session-start.sh         Load project context on session start
│   ├── prompt-submit.sh         Search memories on user prompt
│   ├── pre-tool-use.sh          Pre-write hook (no-op)
│   ├── post-tool-use.sh         Auto-save write/exec operations
│   ├── pre-compact.sh           Snapshot context before compaction
│   ├── post-compact.sh          Reload memories after compaction
│   ├── subagent-start.sh        Load rules for subagents
│   ├── subagent-stop.sh         Save subagent completion
│   ├── stop.sh                  No-op
│   ├── claude-startup.sh        Claude Code session-start hook
│   └── claude-post-tool.sh      Claude Code post-tool-use hook
└── windows/
    ├── hooks.codex.json         Codex hook manifest (powershell, ${USERPROFILE} paths)
    ├── session-start.ps1
    ├── prompt-submit.ps1
    ├── pre-tool-use.ps1
    ├── post-tool-use.ps1
    ├── pre-compact.ps1
    ├── post-compact.ps1
    ├── subagent-start.ps1
    ├── subagent-stop.ps1
    ├── stop.ps1
    ├── claude-startup.ps1
    └── claude-post-tool.ps1
```

## Data Flow

### Memory Save

```
User/Codex "remember: xxx"
        │
        ▼
  memory_save(content, type, tags)
        │
        ▼
  Store::save()
    ├─ m3e-base model → embed_one(content) → 768-dim vector
    ├─ DuckDB INSERT INTO memories (..., embedding FLOAT[768], ...)
    └─ Return mem_{uuid}
```

### Memory Search

```
User/Codex "search memories about coding"
        │
        ▼
  memory_search(query, limit)
        │
        ▼
  Store::search()
    ├─ m3e-base model → embed_one(query) → 768-dim vector
    ├─ search_fts() → BM25 FTS (keyword match)
    │     └─ try_fts_search() → content MATCH ?1
    ├─ search_vector() → list_cosine_similarity(embedding, array_value(...))
    │     └─ ORDER BY score DESC
    └─ Merge (dedup by id), sort by score, return top N
```

## Transports

| Transport | Use Case | Details |
|-----------|----------|---------|
| **Streamable HTTP** | Codex MCP HTTP mode | actix-web on 127.0.0.1:9092, POST `/` for JSON-RPC, GET `/` for health |
| **stdio** | Codex MCP stdio mode | stdin/stdout line-delimited JSON-RPC |

## MCP Protocol Compliance

Supports three protocol versions with version-specific initialize handlers:

| Version | Transport | Capabilities |
|---------|-----------|-------------|
| 2024-11-05 | HTTP+SSE | tools (SSE streaming not implemented) |
| 2025-06-18 | Streamable HTTP | tools |
| 2025-11-25 | Streamable HTTP | tools, ping, version negotiation |

Version negotiation: client sends `protocolVersion` in `initialize` → server matches against supported list → returns matching version or `-32602` error.

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

### Codex Hook Events (all 9 supported)

| Hook | Script | Behavior |
|------|--------|----------|
| SessionStart | `session-start.{sh,ps1}` | Search project memories, cache 1h |
| UserPromptSubmit | `prompt-submit.{sh,ps1}` | Extract prompt keywords, search memories |
| PreToolUse | `pre-tool-use.{sh,ps1}` | No-op placeholder |
| PostToolUse | `post-tool-use.{sh,ps1}` | Auto-save write/exec operations via memory_save |
| PreCompact | `pre-compact.{sh,ps1}` | Snapshot context before compaction |
| PostCompact | `post-compact.{sh,ps1}` | Reload project memories after compaction |
| SubagentStart | `subagent-start.{sh,ps1}` | Load project rules for subagent |
| SubagentStop | `subagent-stop.{sh,ps1}` | Save subagent completion |
| Stop | `stop.{sh,ps1}` | No-op |

### Install Path

- **Unix**: `$HOME/.codex/plugins/agentrete/hooks/` (bash + `hooks.codex.json`)
- **Windows**: `%USERPROFILE%\.codex\plugins\agentrete\hooks\` (PowerShell + `hooks.codex.json`)
- **Claude Code**: `$HOME/.claude/hooks/` (scripts + patched `settings.json`)

### How Hooks Are Packaged

All hook scripts and configuration templates are embedded into the binary at compile time via `include_str!()` in `src/cli/hooks.rs`. The `agentrete setup` command detects the host OS and AI tools, then writes the correct platform scripts to disk.

## Deployment

### systemd User Service

```ini
# ~/.config/systemd/user/agentrete.service
[Service]
ExecStart=/path/to/agentrete mcp --port 9092
Restart=on-failure
RestartSec=2
```

- Auto-starts on boot
- Auto-restarts on crash
- Single process, all Codex instances share

### Startup Sequence

```
systemd → agentrete mcp --port 9092
  ├─ DuckDB open + migration (100ms)
  ├─ m3e-base model load (0-3s, cached)
  ├─ actix-web bind 127.0.0.1:9092
  └─ Ready

Codex start → MCP connect http://127.0.0.1:9092/
  └─ Hooks activate
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `duckdb` | 1.x bundled | Embedded OLAP database |
| `actix-web` | 4.x | HTTP server (streamable HTTP transport) |
| `candle-core` / `candle-transformers` / `candle-nn` | 0.10 | On-device BERT embedding inference |
| `tokenizers` | 0.19 | HuggingFace tokenizer |
| `hf-hub` | 0.5 | HuggingFace model download (fallback) |
| `reqwest` | 0.12 (rustls) | HTTP client for install-model |
| `rmcp` | 1.7 | MCP protocol types (unused currently) |
| `clap` | 4.x | CLI argument parsing |
| `uuid` | 1.x | Memory ID generation |
| `serde` / `serde_json` | 1.x | JSON serialization |
| `tokio` | 1.x | Async runtime |
| `tracing` / `tracing-subscriber` | 0.x | Structured logging |

## Key Design Decisions

1. **DuckDB over SQLite**: FLOAT[] native array type for embedding vectors, no extra extension
2. **actix-web over axum**: Single-threaded Actor model avoids DuckDB `!Sync` issues
3. **m3e-base over bge-m3**: 768d vs 1024d, 391MB vs 2.2GB, good Chinese semantics
4. **list_cosine_similarity over VSS extension**: No extension management, sufficient for <10k memories
5. **Embedding model at startup**: 0s from local cache, visible in INFO log
6. **Version-negotiated MCP**: Clean separation per protocol version, easy to add future versions
7. **Hooks embedded at compile time**: All scripts via `include_str!()`, no external files needed at deploy time

## Embedding Model Comparison (2026-05)

Benchmarked on Ollama server with 5 Chinese/English mixed texts (zh_rule, en_rule, zh_build, en_build, zh_noise, en_noise).

| Model | Dims | Speed | Cross-Lingual | Noise Rejection | Verdict |
|-------|------|-------|---------------|-----------------|---------|
| **granite-embedding:278m** | 768 | 0.1s | 0.77 | 0.48/0.40 | **Default** — balanced |
| qwen3-embedding | 4096 | 0.1s | **0.84** | 0.42/0.32 | Best cross-lingual, poor noise rejection |
| nomic-embed-text-v2-moe | 768 | 1.6s | 0.81 | **0.08/0.06** | Best noise rejection, weak semantics (en_build↔zh_build=0.23) |
| nomic-embed-text | 768 | 0.1s | 0.47 | 0.33 | Poor cross-lingual |
| mxbai-embed-large | 1024 | 0.1s | 0.51 | 0.55 | Poor cross-lingual |

**Cross-Lingual**: cosine similarity between Chinese and English versions of the same rule. Higher is better.  
**Noise Rejection**: cosine similarity between a coding rule and an irrelevant sentence ("what to eat tonight"). Lower is better.

### Recommendation

- **Memory/speed sensitive**: `granite-embedding:278m` (278MB, 768d)
- **Accuracy over all else**: `qwen3-embedding:latest` (7.6B, 4096d) — but tune the similarity threshold
- **Need to filter noise aggressively**: `nomic-embed-text-v2-moe` (768d) — but loses semantic nuance
