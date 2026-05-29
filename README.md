# agentrete

Local-first persistent memory engine for AI coding agents.

**Why**: AI agents lose context between sessions. Agentrete remembers your preferences, project decisions, and past pitfalls — automatically recalled in future conversations.

**How**: Single Rust binary. Embedded DuckDB + on-device embedding model (m3e-base, 391MB). Exposes MCP tools over HTTP or stdio. Cross-platform hooks for Codex CLI, Claude Code, and more.

## Quick Start

```bash
# 1. Build
git clone git@github.com:dyrnq/agentrete.git
cd agentrete
cargo build

# 2. Start MCP server
./target/debug/agentrete daemon install --port 9092
# or: ./target/debug/agentrete mcp --port 9092 &

# 3. Auto-configure AI tools (Codex, Claude, Cursor, etc.)
./target/debug/agentrete setup
```

Or via npm (planned):

```bash
npx agentrete setup
```

## CLI Commands

| Command | Description |
|---------|-------------|
| `save` | Save a memory |
| `search` | Semantic search (BM25 + vector hybrid) |
| `list` | List recent memories |
| `stats` | Database statistics |
| `forget` | Delete by ID |
| `wipe` | Delete all memories |
| `init` | Initialize project |
| `doctor` | Run diagnostics |
| `setup` | Auto-detect AI tools and configure MCP + hooks |
| `daemon` | OS-native background service (systemd/launchd) |
| `mcp` | Start MCP server (HTTP or stdio) |

## MCP Tools

| Tool | Description |
|------|-------------|
| `memory_search` | Semantic search (BM25 + vector hybrid) |
| `memory_save` | Save memory with duplicate detection |
| `memory_list` | List recent memories |
| `memory_forget` | Delete by ID |
| `memory_stats` | Database statistics |

## Features

- **Embedded**: Single binary, no external DB or API required
- **Semantic search**: 768-dim vector search via m3e-base + DuckDB
- **Cross-platform**: Linux, macOS, Windows (native PowerShell hooks)
- **MCP protocol**: 2024-11-05, 2025-06-18, 2025-11-25 with version negotiation
- **Hooks**: 9 Codex events + 2 Claude Code events, all embedded at compile time
- **Model auto-download**: First `save`/`search` downloads embedding model lazily
- **8 agents supported**: Codex CLI, Claude Code, Cursor, Zed, OpenCode, Windsurf, Goose, Gemini CLI

## Architecture

```
Codex / Claude Code / Cursor / Zed / ...
        │
        ▼
  agentrete MCP (HTTP :9092 or stdio)
        │
        ▼
  ┌──────────┬──────────┬──────────┐
  │ v2024    │ v2025_06 │ v2025_11 │  ← version-negotiated handlers
  └──────────┴──────────┴──────────┘
        │
        ▼
  DuckDB + BM25 FTS + m3e-base embedding
```

## Memory Lifecycle

1. **Save** — text → 768-dim vector → DuckDB
2. **Search** — BM25 keywords + vector cosine similarity → ranked results
3. **Forget** — delete by ID

## Configuration

| Env Var | Default | Description |
|---------|---------|-------------|
| `AGENTRETE_URL` | `http://127.0.0.1:9092` | MCP server URL |
| `AGENTRETE_MODEL` | `moka-ai/m3e-base` | Embedding model |
| `AGENTRETE_NO_EMBED` | — | Disable embedding (BM25 only) |
| `HF_ENDPOINT` | `https://hf-mirror.com` | HuggingFace mirror |

## Docs

- [Architecture](docs/architecture.md)
- [Agent Hooks Reference](docs/agent-hooks.md)
- [Memory Decision Guide](docs/memory-decision.md)
- [Memory Lifecycle](docs/memory-lifecycle.md)
- [DuckDB Concurrency](docs/duckdb-concurrency.md)
