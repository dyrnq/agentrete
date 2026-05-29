# Agentrete Memory Lifecycle

## Overview

Agentrete is a local embedded memory engine that provides AI coding assistants (Codex CLI, Claude Code, etc.) with cross-session long-term memory.

Core goal: **Let AI remember your preferences, project technical decisions, and past pitfalls — automatically recalled in future conversations.**

## Architecture

```
┌──────────────────────────────────────────────────────┐
│               Codex CLI / Claude Code                 │
│                                                      │
│  SessionStart ──→ agentrete hooks (search context)    │
│  UserPrompt   ──→ memory_search (recall)              │
│  PostToolUse  ──→ memory_save (auto-record)           │
│  "remember"   ──→ Codex/Claude calls memory_save      │
└──────────────────────┬───────────────────────────────┘
                       │ HTTP :9092
                       ▼
┌──────────────────────────────────────────────────────┐
│                  agentrete MCP Server                 │
│                                                      │
│  actix-web                                            │
│  ┌─────────┐  ┌─────────┐       ┌──────────────┐    │
│  │ memory_  │  │ memory_  │       │  DuckDB      │    │
│  │ search   │  │ save     │ ─────→│  • memories  │    │
│  │ list     │  │ stats    │       └──────┬───────┘    │
│  └─────────┘  └─────────┘              │            │
│                                         │            │
│  ┌──────────────────────────────────────┘            │
│  │  Embedding model (m3e-base, 768d, 391MB)         │
│  │  text → vector → list_cosine_similarity           │
│  └───────────────────────────────────────────────────┘
└──────────────────────────────────────────────────────┘
```

## 1. Acquiring Memories

Memories enter the system through three paths:

### Path A: Hook Auto-Record

Codex/Claude's `PostToolUse` hook fires after each tool call, auto-saving write operations.

```
User: "change the port"
  → Agent executes Edit tool
    → PostToolUse hook fires
      → post-tool-use.{sh,ps1} reads JSON payload
        → filters read-only operations (Read/Glob/Grep skipped)
          → curl POST memory_save(type=fact, tags=hook,tool-call)
```

**Not auto-recorded**:
- User messages themselves
- Read-only operations (Read, Glob, Grep)
- Temporary interactions

### Path B: Agent Autonomous Decision

AGENTS.md instructs the agent to search memories at the start of conversation and proactively call `memory_save` when information is judged valuable.

### Path C: User Explicit

```
User says "remember xxx" 
  → Agent calls memory_save
```

## 2. Storing Memories

Each memory is stored in DuckDB with structure:

```sql
CREATE TABLE memories (
    id              VARCHAR PRIMARY KEY,     -- mem_{uuid}
    type            VARCHAR,                 -- rule | decision | pattern | bug | fact
    content         TEXT NOT NULL,           -- memory content
    tags            VARCHAR[],               -- ["rust","compilation"]
    project         VARCHAR,                 -- project name
    importance      FLOAT DEFAULT 0.5,
    embedding       FLOAT[],                 -- 768d vector (m3e-base model)
    embedding_model VARCHAR,                 -- "m3e-base"
    embedding_dims  INTEGER,                 -- 768
    created_at      TIMESTAMP,
    updated_at      TIMESTAMP
);
```

**On write**:
1. text → candle loads m3e-base → 768-dim vector
2. DuckDB INSERT INTO memories

## 3. Searching Memories

Hybrid search = BM25 full-text + vector semantic:

```
Search "coding standards"
  │
  ├─ Phase 1: BM25 FTS (keyword match)
  │   └─ Finds records containing "coding" 
  │
  ├─ Phase 2: Vector semantic search (list_cosine_similarity)
  │   ├─ query text → embedding 768d vector
  │   ├─ cosine similarity against each memory
  │   └─ Finds semantically similar records
  │
  └─ Merge results → sort by score → return top N
```

## 4. Memory Lifecycle

```
┌──────────┐     ┌──────────┐     ┌──────────┐
│  SAVE    │ ──→ │  INDEX   │ ──→ │  SEARCH  │
│  write DB│     │ embedding│     │ hybrid   │
└──────────┘     └──────────┘     └──────────┘
                                       │
                                  Manual FORGET
                                       │
                                  ┌────▼─────┐
                                  │  DELETE   │
                                  │  from DB  │
                                  └──────────┘
```

- **No auto-expiry**: Memories persist until manually deleted
- **Deletion**: `agentrete forget {id}` or MCP `memory_forget` tool
- **Cleanup advice**: `agentrete list` to review, `agentrete forget` to remove noise

## 5. Cross-Session Flow

```
Session 1                    Session 2 (new project, new instance)
─────────                    ─────────
"Never use sed to edit"      
  → memory_save(type=rule)   
                              SessionStart hook → search "project rules"
                                → finds "[rule] Never use sed" (score=0.92)
                              Agent uses apply_patch instead of sed ✅
```

## 6. Technical Details

| Component | Choice | Reason |
|-----------|--------|--------|
| Database | DuckDB | Embedded OLAP, SQL-friendly, native FLOAT[] |
| Vector search | `list_cosine_similarity` | DuckDB built-in, no extensions |
| Full-text search | FTS (BM25) | DuckDB built-in |
| Embedding model | m3e-base | Good Chinese semantics, 768d, 391MB |
| HTTP framework | actix-web | Actor model, avoids DuckDB `!Sync` issues |
| MCP protocol | Hand-written JSON-RPC | Streamable HTTP 2025-11-25 compliant |
| Hooks | bash + curl / PowerShell | Codex & Claude hook mechanisms |

## 7. File Layout

```
$HOME/.agentrete/memory.db              ← DuckDB data file
$HOME/.cache/huggingface/hub/           ← Model cache
  models--moka-ai--m3e-base/
    snapshots/main/model.safetensors      ← 391MB

$HOME/.codex/
  config.toml                            ← MCP server configuration
  plugins/agentrete/hooks/               ← Hook scripts
$HOME/.claude/
  hooks/                                 ← Claude Code hook scripts
  settings.json                          ← Hook + MCP configuration
```
