# Agentrete vs. Peer Projects

Comparison of agentrete with other AI coding agent memory/context systems.
Last updated: 2026-05-30.

## agentrete vs. mempal — Head-to-Head

Based on source-level analysis of mempal (`.dev/mempal`, `sqlite` branch) and agentrete as of May 30, 2026.

| Dimension | agentrete (current) | mempal |
|-----------|---------------------|--------|
| **Storage** | SQLite + FTS5 + **vec0 standalone** (`ext/*.so` embedded via `include_bytes!()`) | SQLite + **sqlite-vec crate** (C source compiled via `cc` → `libsqlite_vec0.a`, auto-extension on every connection) |
| **Search** | **RRF fusion**: vec0 KNN + FTS5 BM25 concurrent → `1/(K+rank)` merge + temporal decay | **RRF fusion**: BM25 + vector in parallel, merged via `1/(60+rank)` |
| **Embedding** | model2vec-rs / **Ollama / OpenAI / Anthropic** (4 backends) | model2vec-rs / ONNX (2 backends, no remote API) |
| **MCP tools** | **6** (search/save/list/forget/stats/compact) | **23** (incl. KG, fact_check, doctor, brief, context, partner, tunnels, ingest, delete, taxonomy) |
| **Knowledge graph** | ❌ | ✅ SPO triples + `mempal_kg` query |
| **Cross-project (tunnels)** | ❌ | ✅ tunnels (cross-wing links) |
| **Protocol self-description** | ❌ | ✅ `MEMORY_PROTOCOL` (~400 lines) embedded in `initialize.instructions` |
| **Multi-agent collaboration** | ❌ | ✅ `peek_partner` + `cowork_push` + `cowork_bus` |
| **Citation traceability** | ✅ `source_file` field in schema + search results | ✅ `drawer_id` + `source_file` + `trigger_hints` |
| **Cognitive brief** | ❌ | ✅ `mempal_brief` (facts + evidence + uncertainty + cards, deterministic) |
| **Self-diagnostics** | ❌ | ✅ `mempal_doctor` (schema version, tool count, DB size, runtime health) |
| **Install** | `cargo build` (single binary ~350MB debug) | `cargo install` (feature flags: model2vec/onnx/rest) |
| **Web framework** | **axum 0.8.9** + sqlx async pool | **rmcp** (MCP-native framework) + rusqlite |
| **Config** | config-rs (TOML/YAML/JSON, nested `[embedding.*]`) | Custom config |
| **Model distillation** | ✅ Full toolchain (bge-small distilled to 10MB, docs/model2vec-distillation.md) | ❌ No distillation tools |
| **Embed worker** | ✅ Async batch embed + model-switch auto-recompute (detect dim change → clear → re-embed) | ❌ Compute on demand |
| **Seed subcommand** | ✅ Rules baked into binary, writes directly to SQLite | ❌ |
| **PreToolUse hook** | ✅ Blocks sed/python3 source tampering | ✅ MCP hooks |
| **Cross-platform vec0** | ✅ linux/macos/windows × x86_64/aarch64 all bundled | sqlite-vec crate handles via C compilation |

### Key differences explained

**Search algorithm**: agentrete uses serial fallback (try vec0 KNN first; if empty/error, fall through to cosine rerank; if no embedder, fall through to FTS5 BM25). mempal runs BM25 and vector in parallel, then fuses the two ranked lists with Reciprocal Rank Fusion (`RRF_K=60`). RRF is theoretically more principled — a result appearing in both lists gets boosted. However, agentrete's serial chain is faster when vec0 KNN returns good results (no need to run the second path).

**vec0 loading**: mempal uses the `sqlite-vec` Rust crate, which compiles `sqlite-vec.c` from source via `cc` and registers it as a `rusqlite::auto_extension`. Every `Connection::open()` automatically has `vec0` available — no manual `load_extension()` needed. agentrete pre-downloads 6 platform-specific `.so`/`.dylib`/`.dll` files and calls `load_extension()` at startup. The crate approach is cleaner and avoids bundling multi-platform binaries, but requires a C compiler at build time.

**MCP tool count (6 vs 23)**: Most of mempal's extra tools are in domains agentrete deliberately doesn't cover — knowledge graphs, multi-agent cowork bus, 4-tier knowledge lifecycle (distill/gate/promote/demote/publish). The ones genuinely worth borrowing: `mempal_doctor` (self-diagnostics), `MEMORY_PROTOCOL` (teach agents how to use memory), and RRF fusion.

**Embedding backends**: agentrete wins with 4 backends vs mempal's 2. mempal has no remote API support (no Ollama/OpenAI/Anthropic embedding clients), only local model2vec and ONNX.

## Overview vs. Other Peers

| Feature | agentrete | mempal | Superpowers | Karpathy Skills |
|---------|-----------|--------|-------------|-----------------|
| **Type** | MCP server | MCP server | Skills + instructions | CLAUDE.md |
| **Persistence** | SQLite + vec0 | SQLite + sqlite-vec | File-based skills | File-based |
| **Search** | vec0 KNN → cosine → FTS5 | BM25 + vec0 RRF | N/A | N/A |
| **Embedding** | model2vec / Ollama / OpenAI / Anthropic | model2vec / ONNX | None | None |
| **Auto-save** | MCP hooks | MCP hooks | None | None |
| **Cross-agent** | 8 agents | MCP-generic | 7 agents | Claude only |
| **Self-hosted** | ✅ | ✅ | ✅ | ✅ |
| **Privacy** | All local | All local | Local | Local |

## Open-Source Memory Alternatives

### AgentMem

**Type**: MCP server (Rust, LanceDB + ONNX)

| Feature | agentrete | AgentMem |
|---------|-----------|----------|
| Storage | SQLite + vec0 | LanceDB |
| Embedding | Model2Vec (10MB) / Ollama / OpenAI | ONNX (80-274MB) |
| Search | vec0 KNN → FTS5 cosine → FTS5 BM25 | BM25 + vector RRF + cross-encoder rerank |
| Cross-agent hooks | Codex, Claude, Cursor, 5+ more | Claude, Codex, Gemini, OpenClaw, Hermes |
| Auto-capture | MCP post-tool-use hooks | Automatic indexing + file watcher |
| Temporal decay | No | Yes (configurable half-life) |
| OpenTelemetry | No | Yes |

### ramem (RAM)

**Type**: MCP server (Rust, custom indexing)

| Feature | agentrete | ramem |
|---------|-----------|-------|
| Approach | Explicit save + background embed worker | Auto-index transcripts + explicit memorize |
| Search backend | sqlite-vec (KNN) | LanceDB (vector) |
| Embedding models | Model2Vec / Ollama / OpenAI / Anthropic | ONNX local only |
| Hooks integration | MCP hooks (post-tool-use) | File watcher + MCP hooks |
| Use case | Coding rules, decisions, patterns | Session transcripts, decisions |

### agentmemory

**Type**: MCP server (TypeScript, iii-engine, Node.js)

| Feature | agentrete | agentmemory |
|---------|-----------|-------------|
| Language | Rust (single binary) | TypeScript (Node.js + npx) |
| Storage | SQLite + vec0 | iii-engine (proprietary) |
| Embedding | Model2Vec (10MB local) / Ollama / OpenAI / Anthropic | OpenAI / Ollama / Gemini / Claude |
| Search | vec0 KNN → FTS5 cosine → FTS5 BM25 | BM25 + vector + graph RRF fusion |
| Memory evolution | No (flat) | Yes (versioning, supersession) |
| Knowledge graph | No | Yes (entities + BFS) |
| Auto-forgetting | No | Yes (TTL, contradiction) |
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
- Fastest search (vec0 KNN ~0.1ms vs LanceDB/RRF)
- Most embedding backends (4)
- Most agent integrations (8)
- Coding-specific memory types (rule/decision/pattern/bug/fact)

## Feature Borrowing Analysis

Features worth borrowing from peer projects, prioritized by value/effort ratio.
Status as of 2026-05-30 — most High ROI features already implemented.

### High ROI (implement soon)

| Feature | Source | Status | How agentrete implements it |
|---------|--------|--------|-----------------------------|
| **RRF fusion for search** | mempal | ✅ Done | `rrf_merge()` in `storage.rs` — merges vec0 KNN + FTS5 BM25 via `1/(K+rank)` (K=60) |
| **MEMORY_PROTOCOL in instructions** | mempal | ✅ Done | `MEMORY_PROTOCOL` (~80 lines) returned in `initialize.instructions` across all 3 MCP versions |
| **mempal_doctor equivalent** | mempal | ✅ Done | `memory_stats` MCP tool — schema version, type counts, model info, vec0 status, DB size |
| **Temporal decay in search** | ramem, agentmemory | ✅ Done | `decay_score()` — `score * e^(-days/half_life)`, `half_life_days` configurable (default 90) |
| **memory_list filtering by type** | AgentMem | ✅ Done | `memory_list` accepts optional `type` param, passed as `WHERE type=?` |

### Medium ROI (next milestone)

| Feature | Source | Effort | Why |
|---------|--------|--------|-----|
| **Auto-capture via file watcher** | ramem | Medium | `notify` crate; no MCP hooks needed for file changes |
| **Privacy filter (strip secrets)** | agentmemory | Medium | Regex patterns for API keys, tokens, passwords |
| **Cross-encoder reranking** | ramem | Medium | Improve accuracy for ambiguous queries |
| **Embedding model embed into binary** | — | Medium | `include_bytes!()` model2vec (~10MB) into binary; current: auto-download on first use |
| **FTS5 Chinese tokenizer** | — | Medium | Replace `unicode61` with `jieba`/`icu` tokenizer for proper CJK search |

### Low ROI (defer)

| Feature | Source | Why skip for now |
|---------|--------|-----------------|
| Knowledge graph | mempal, agentmemory | Memory volume too low; petgraph viable later |
| 4-tier memory consolidation | agentmemory | Overkill for coding rules |
| OpenTelemetry | ramem | Only if multi-instance deployment needed |
| Team/shared memory | agentmemory | Solo use case currently |
| Auto-forgetting (TTL) | agentmemory | Coding rules don't expire |
| Multi-agent collaboration | mempal | Single-agent use case |
| Cognitive brief | mempal | Nice-to-have, not core need |
| Privacy filter | agentmemory | Low risk for local-first tool; can add later |
| Cross-encoder reranking | ramem | vec0 KNN + cosine rerank already high accuracy for coding queries |

### Now-borrowed (from mempal, already shipped)

The following features originated as deliberate borrows from mempal and are now core:

1. **RRF fusion** — `rrf_merge()` + concurrent vec0/FTS5 join via `tokio::join!`
2. **MEMORY_PROTOCOL** — `protocol.rs` with 7 sections, returned in all MCP initialize responses
3. **memory_stats** — type counts, model info, DB health, schema version, vec0 status (a la mempal_doctor)
4. **Temporal decay** — `e^(-days/half_life)` factor on search scores (half_life=90d default)
5. **memory_list type filter** — optional `type` param filters by memory category
6. **async embed worker** — background loop with configurable batch size/poll interval (mempal does compute-on-demand)
7. **Hooks integration** — 9 Codex events + 2 Claude Code hooks, all embedded at compile time
8. **Cross-platform vec0** — 6 platform-specific binaries, extracted + loaded at startup

## Competitive Advantages (Keep)

| Advantage | vs. |
|-----------|-----|
| **Smallest model (10MB Model2Vec)** | AgentMem (80MB ONNX), ramem (80-274MB ONNX) |
| **Fastest search (vec0 KNN ~0.1ms)** | agentmemory (BM25+vector+graph RRF), ramem (RRF+rerank) |
| **Most backends (Model2Vec/Ollama/OpenAI/Anthropic)** | AgentMem (ONNX only), ramem (ONNX only), mempal (model2vec/ONNX only) |
| **Rust single binary** | agentmemory (Node.js+npm), AgentMem (Rust+LanceDB) |
| **Most agent integrations (8)** | AgentMem (5), ramem (MCP-generic) |
| **vec0 KNN (native in SQLite)** | All others use external vector DBs or LanceDB |
| **Model distillation toolchain** | mempal (no distillation), ramem (no distillation) |

## Ecosystem Position

| Layer | Tool | Purpose |
|-------|------|---------|
| Methodology | Superpowers / OpenSpec | How to structure development |
| Memory | **agentrete** | What we learned, rules, decisions |
| Guidelines | Karpathy Skills / AGENTS.md | Coding anti-patterns, style |
| Agent | Codex / Claude / Cursor | Execution |
