# Agentrete vs. Peer Projects

Comparison of agentrete with other AI coding agent memory/context systems.

## Overview

| Feature | agentrete | Superpowers (obra) | Karpathy Skills | Cursor Rules | Claude Memory |
|---------|-----------|-------------------|-----------------|-------------|---------------|
| **Type** | MCP server (long-term memory) | Skills + instructions | CLAUDE.md guidelines | `.cursorrules` file | Built-in (closed) |
| **Persistence** | SQLite (cross-session) | File-based skills | File-based | File-based | Proprietary |
| **Search** | vec0 KNN + FTS5 hybrid | N/A (skill trigger) | N/A | N/A | Semantic (closed) |
| **Embedding** | Model2Vec (local) / Ollama / OpenAI / Anthropic | None | None | None | Proprietary |
| **Auto-save** | MCP hooks (post-tool-use) | None | None | None | Automatic |
| **Cross-agent** | Codex + Claude Code + Cursor + Zed + OpenCode + Windsurf + Goose + Gemini | Codex + Claude Code + Cursor + Factory + Gemini + OpenCode + Copilot | Claude Code only | Cursor only | Claude only |
| **Self-hosted** | ✅ (single binary, SQLite) | ✅ (git clone) | ✅ (git clone) | ✅ (file) | ❌ |
| **Privacy** | All local, no telemetry | Local | Local | Local | Cloud |

## Detailed Comparison

### agentrete

**Strengths**:
- True long-term memory with semantic search (vec0 KNN + cosine + FTS5)
- Automatic memory capture via MCP hooks
- Cross-session persistence (rules, decisions, patterns survive restarts)
- Multi-backend embeddings: local Model2Vec (10MB, 0.17ms), remote Ollama/OpenAI/Anthropic
- Single binary deploy, zero external dependencies
- 8 agent integrations (Codex, Claude, Cursor, etc.)

**Weaknesses**:
- Requires MCP server running (process management needed)
- Model2Vec requires one-time distillation step
- No built-in skill/methodology system (complementary to Superpowers)

### Superpowers (obra)

**Strengths**:
- Complete development methodology (spec → plan → subagent → review)
- Subagent-driven development with autonomous 2-hour sessions
- TDD-focused workflow
- Excellent for complex, multi-step development tasks

**Weaknesses**:
- No persistent memory between sessions
- Skills are methodology-focused, not memory-focused
- No semantic search over past decisions/patterns

### Karpathy Skills

**Strengths**:
- Addresses common LLM pitfalls (over-engineering, assumption-making)
- Simple, single-file `CLAUDE.md` integration
- Clear anti-patterns and coding rules

**Weaknesses**:
- Claude Code only
- No semantic search
- No cross-session persistence
- Static rules, no learning from experience

### Complementary Use

Agentrete and Superpowers can be used together:

```
Superpowers (methodology)  +  agentrete (memory)
        │                           │
    "How to build"              "What we learned"
     spec → plan                 rules → decisions
     subagent-driven             patterns → bugs
```

**Recommended setup**:
1. Install Superpowers skills for development methodology
2. Install agentrete for cross-session memory
3. Use Karpathy-inspired `CLAUDE.md` for anti-pattern rules
4. Seed agentrete with community rules from Superpowers + Karpathy


## Open-Source Memory Alternatives

### AgentMem

**Type**: MCP server (Rust, LanceDB + ONNX)

**Strengths**:
- Embedded LanceDB (no external DB needed)
- ONNX embeddings (local, 80-274MB models)
- Hybrid search (BM25 + vector RRF fusion)
- Cross-encoder reranking for better accuracy
- CLI parity with MCP tools

**vs. agentrete**:
| Feature | agentrete | AgentMem |
|---------|-----------|----------|
| Storage | SQLite + sqlite-vec | LanceDB |
| Embedding | Model2Vec (10MB) / Ollama / OpenAI | ONNX (80-274MB) |
| Search | vec0 KNN → FTS5 cosine → FTS5 BM25 | BM25 + vector RRF + cross-encoder rerank |
| Cross-agent hooks | Codex, Claude, Cursor, 5+ more | Claude, Codex, Gemini, OpenClaw, Hermes |
| Auto-capture | MCP post-tool-use hooks | Automatic indexing + file watcher |
| Temporal decay | No | Yes (configurable half-life) |
| OpenTelemetry | No | Yes |
| Binary size | ~350MB | Unknown (LanceDB + ONNX) |

### ramem (RAM)

**Type**: MCP server (Rust, custom indexing)

**Strengths**:
- Zero infrastructure, embedded LanceDB
- Automatic file watcher for session transcripts
- OpenTelemetry observability
- Async enrichment pipeline
- Explicit memorize + auto-index hybrid

**vs. agentrete**:
| Feature | agentrete | ramem |
|---------|-----------|-------|
| Approach | Explicit save + background embed worker | Auto-index transcripts + explicit memorize |
| Search backend | sqlite-vec (KNN) | LanceDB (vector) |
| Embedding models | Model2Vec / Ollama / OpenAI / Anthropic | ONNX local only |
| Hooks integration | MCP hooks (post-tool-use) | File watcher + MCP hooks |
| Use case | Coding rules, decisions, patterns | Session transcripts, decisions |


### agentmemory

**Type**: MCP server (TypeScript, iii-engine, Node.js)

**Strengths**:
- Most popular memory MCP (1200+ GitHub stars on design doc)
- Triple-stream retrieval: BM25 + vector + knowledge graph with RRF fusion
- 4-tier memory consolidation (observations → memories → facts → insights)
- Memory evolution: versioning, supersession, relationship graphs
- Auto-forgetting: TTL expiry, contradiction detection, importance eviction
- Privacy: auto-strips API keys, secrets, `<private>` tags
- Self-healing: circuit breaker, provider fallback chain, health monitoring
- Knowledge graph with entity extraction + BFS traversal
- Git snapshots for version/rollback/diff
- 9+ agent integrations

**vs. agentrete**:
| Feature | agentrete | agentmemory |
|---------|-----------|-------------|
| Language | Rust (single binary) | TypeScript (Node.js + npx) |
| Storage | SQLite + sqlite-vec | iii-engine (proprietary) |
| Embedding | Model2Vec (10MB local) / Ollama / OpenAI / Anthropic | OpenAI / Ollama / Gemini / Claude |
| Search | vec0 KNN → FTS5 cosine → FTS5 BM25 | BM25 + vector + graph RRF fusion |
| Memory evolution | No (flat) | Yes (versioning, supersession) |
| Knowledge graph | No | Yes (entities + BFS) |
| Auto-forgetting | No | Yes (TTL, contradiction) |
| Binary size | ~350MB (with jemalloc) | N/A (Node.js runtime) |
| Install | `cargo build` or prebuilt binary | `npm i -g @agentmemory/mcp` |
| Target user | Developers who want speed + simplicity | Teams who want advanced memory management |


## Market Positioning

```
                          Explicit Memory
                               ▲
                    agentrete  │  AgentMem
                    (coding    │  (general
                     focused)  │   purpose)
                               │
        Lightweight ◄──────────┼──────────► Heavy
                               │
                    Superpowers│  ramem
                    (method-   │  (session
                     ology)    │   transcripts)
                               ▼
                          Implicit Memory
```

**agentrete** occupies the **lightweight + explicit** quadrant:
- Smallest model (10MB Model2Vec vs 80MB ONNX)
- Fastest search (vec0 KNN 0.1ms vs LanceDB/RRF)
- Most agent integrations (8)
- Coding-specific memory types (rule/decision/pattern/bug/fact)


## Feature Borrowing Analysis

Features worth borrowing from peer projects, prioritized by value/effort ratio.

### High ROI (implement soon)

| Feature | Source | Effort | Why agentrete needs it |
|---------|--------|--------|----------------------|
| **memory_save_batch** | all (batch ops) | Low (add MCP tool + SQL) | Already in TODO; reduces HTTP round-trips 100x |
| **Temporal decay in search** | ramem, agentmemory | Low (multiply score by `e^(-days/half_life)`) | Old rules shouldn't outrank recent decisions |
| **memory_list filtering by type** | AgentMem (namespaces) | Low (add WHERE type=? to list()) | Already needed — can't list only "rule" type today |
| **Git snapshot of memory state** | agentmemory | Medium (dump DB to git) | Rollback after bad memory saves; audit trail |

### Medium ROI (next milestone)

| Feature | Source | Effort | Why |
|---------|--------|--------|-----|
| **Auto-capture via file watcher** | ramem (FS watcher) | Medium (notify crate) | No MCP hooks needed — detect file changes → save observations |
| **Privacy filter (strip secrets)** | agentmemory | Medium (regex patterns) | Safety: never store API keys, tokens, passwords |
| **RRF fusion for search** | ramem, agentmemory | Medium (algorithm) | Merge vec0 + FTS5 scores better than current fallback chain |
| **Cross-encoder reranking** | ramem | Medium (need model) | Improve search accuracy for ambiguous queries |
| **Session persistence** | agentmemory (4-tier) | High | Summarize sessions → extract patterns → long-term insight |

### Low ROI (defer)

| Feature | Source | Why skip for now |
|---------|--------|-----------------|
| Knowledge graph | agentmemory | Memory volume too low (32 items); petgraph viable later |
| 4-tier memory consolidation | agentmemory | Overkill for coding rules; flat rules/decisions/patterns sufficient |
| OpenTelemetry | ramem | Only if multi-instance deployment needed |
| Team/shared memory | agentmemory | Solo use case currently |
| Cross-encoder rerank | ramem | Model2Vec accuracy sufficient; trade speed for accuracy only when needed |
| Auto-forgetting (TTL) | agentmemory | Coding rules don't expire; contradiction detection more useful |
| Claude MEMORY.md bridge | agentmemory | Claude-specific; agentrete is agent-agnostic |

## Competitive Advantages (Keep)

What agentrete does better than peers — don't dilute these:

| Advantage | vs. |
|-----------|-----|
| **Smallest model (10MB Model2Vec)** | AgentMem (80MB ONNX), ramem (80-274MB ONNX) |
| **Fastest search (vec0 KNN 0.1ms)** | agentmemory (BM25+vector+graph RRF), ramem (RRF+rerank) |
| **Most backends (Model2Vec/Ollama/OpenAI/Anthropic)** | AgentMem (ONNX only), ramem (ONNX only) |
| **Rust single binary (215MB debug, ~60MB release)** | agentmemory (Node.js+npm), AgentMem (Rust+LanceDB) |
| **Most agent integrations (8)** | AgentMem (5), ramem (MCP-generic) |
| **sqlite-vec KNN (native)** | All others use external vector DBs |

## Ecosystem Position

| Layer | Tool | Purpose |
|-------|------|---------|
| Methodology | Superpowers / OpenSpec | How to structure development |
| Memory | **agentrete** | What we learned, rules, decisions |
| Guidelines | Karpathy Skills / AGENTS.md | Coding anti-patterns, style |
| Agent | Codex / Claude / Cursor | Execution |
