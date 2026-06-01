# agentrete

Local-first persistent memory engine for AI coding agents.

**Why**: AI agents lose context between sessions. Agentrete remembers your preferences, project decisions, and past pitfalls — automatically recalled in future conversations.

**How**: Single Rust binary. Embedded SQLite + sqlite-vec KNN + Model2Vec (131MB minilm-256d default). Exposes MCP tools over HTTP or stdio. Cross-platform hooks for Codex CLI, Claude Code, and more.

## Quick Start

### npm (recommended)

```bash
# 1. Install hooks + configure AI tools
npx agentrete setup

# 2. Start background service
npx agentrete daemon install

# 3. Verify
npx agentrete stats
```

**Setup** auto-detects your AI tools (Codex, Claude, Cursor, etc.), writes MCP config, and installs hooks.

**Daemon** registers a systemd/launchd service that auto-starts on boot.

**Stats** should show `Memories: 0` — ready. Restart your AI tool and start working.

### From source (developers)

```bash
git clone git@github.com:dyrnq/agentrete.git
cd agentrete
cargo build
./target/debug/agentrete daemon install --port 9092
./target/debug/agentrete setup
```

### After install

1. Restart your AI tool
2. Hooks auto-search relevant memories on session start
3. Use `memory_save` to persist decisions, patterns, bugs
4. Run `memory_stats` to see what's stored

> **Want semantic search?** Add a model to `~/.agentrete/config.toml`:
> ```toml
> [embedding]
> backend = "model2vec"
> [embedding.model2vec]
> model = "minilm-256d"
> dims = 256
> ```
> Without this, agentrete uses fast BM25 text search — no model download needed.

## CLI Commands

| Command | Description |
|---------|-------------|
| `save` | Save a memory |
| `search` | Semantic search (vec0 KNN + FTS5 BM25 → RRF fusion) |
| `list` | List recent memories |
| `stats` | Database statistics |
| `forget` | Delete by ID |
| `wipe` | Delete all memories |
| `init` | Initialize project |
| `doctor` | Run diagnostics |
| `setup` | Auto-detect AI tools and configure MCP + hooks |
| `daemon` | OS-native background service (systemd/launchd) |
| `mcp` | Start MCP server (HTTP or stdio) |
| `scan` | Scan codebase and build knowledge graph |

## MCP Tools

| Tool | Description |
|------|-------------|
| `memory_search` | Semantic search (vec0 KNN + FTS5 BM25 → RRF fusion + temporal decay) |
| `memory_save` | Save memory with auto-detect project from git, dry_run preview |
| `memory_list` | List recent memories, optionally filtered by type |
| `memory_forget` | Delete by ID |
| `memory_stats` | DB statistics (schema version, type counts, model info, vec0 status) |
| `memory_compact` | Deduplicate (exact or semantic by cosine threshold) + reclaim disk |
| `kg_query` | Knowledge graph query (neighbors, path, subgraph by predicate/direction/project) |
| `kg_scan` | Start background codebase scan with ast-grep (incremental via file hash cache) |
| `kg_scan_status` | Check if a background scan is running |

## Features

- **Embedded**: Single binary, no external DB or API required
- **Semantic search**: 256-1024d vector search via Model2Vec + sqlite-vec KNN, hybrid RRF fusion with FTS5 BM25
- **Knowledge graph**: SPO triple store (petgraph + SQLite), incremental codebase scan via ast-grep (16 languages) with per-file hash cache, force re-scan support
- **Cross-platform**: Linux, macOS, Windows — all with native hooks (bash/PowerShell)
- **MCP protocol**: 2024-11-05, 2025-06-18, 2025-11-25 with version negotiation
- **Hooks**: 9 Codex events + 2 Claude Code events, all embedded at compile time
- **Model distillation**: 9 sentence-transformers models distillable to Model2Vec (10-497MB)
- **8 agents supported**: Codex CLI, Claude Code, Cursor, Zed, OpenCode, Windsurf, Goose, Gemini CLI

## Architecture

```
Codex / Claude Code / Cursor / Zed / ...
        │
        ▼
  agentrete MCP (HTTP :9092 or stdio)
        │
        ├── Memory Engine ── SQLite + FTS5 + vec0 KNN + model2vec
        │                      rules/decisions/patterns/bugs (semantic search)
        │
        └── Knowledge Graph ── SQLite kg_triples + petgraph (in-memory)
                               code scan via ast-grep (16 languages)
                               kg_query / kg_scan (with optional watch)
```

## Memory Lifecycle

### Sessions & Observations
- **Session auto-create** — new session recorded on MCP `initialize`
- **Observations** — `memory_save` / `memory_search` calls auto-logged to observations table
- **stop.sh hook** — reminds AI to save key decisions every 10th session stop

### Memory Engine (always on)
1. **Save** — text + metadata → SQLite (embedding=NULL, embed worker picks up async)
2. **Embed** — background worker batches pending rows → Model2Vec/remote API → writes embedding + vec0 index
3. **Search** — query → vec0 KNN + FTS5 BM25 concurrent → RRF fusion → temporal decay → ranked results
4. **Forget** — hard delete (row + vec0 entry)

### Knowledge Graph (optional, enabled via config)
1. **Scan** — `agentrete scan .` or `kg_scan` MCP → ast-grep scans codebase → SPO triples stored in SQLite
2. **Watch** — `kg_scan` with `watch: true` → automatic re-scan on file changes (incremental via hash cache)
3. **Query** — `kg_query` → petgraph in-memory traversal (neighbors, shortest path, filtered by predicate/direction/project)

## Configuration

Configuration via `~/.agentrete/config.toml` (TOML/YAML/JSON, env override with `AGENTRETE__*` prefix):

```toml
port = 9092

[embedding]
backend = "model2vec"   # "none" | "model2vec" | "remote"

[embedding.model2vec]
model = "minilm-256d"
dims = 256

# [embedding.remote]
# url = "http://localhost:11434"
# model = "qwen3-embedding:latest"
```

See [config-reference.toml](docs/config-reference.toml) for all options.

## Docs

- [Architecture](docs/architecture.md)
- [Agent Hooks Reference](docs/agent-hooks.md)
- [Memory Decision Guide](docs/memory-decision.md)
- [Memory Lifecycle](docs/memory-lifecycle.md)
