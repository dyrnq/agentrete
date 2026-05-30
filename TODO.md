# Agentrete TODO

## High Priority

- [ ] **Model2Vec embed into binary** — `include_bytes!()` model2vec model (~50MB) into binary for zero-config local embedding. M2V_multilingual_output already downloaded. Temp-dir extract + StaticModel::from_bytes().
## Medium Priority

- [ ] **model2vec distiller** — `agentrete distill` subcommand (uv + model2vec[distill]).

## Low Priority

- [ ] **FTS5 Chinese tokenizer** — `unicode61` does single-char split. `cwt/fts5-icu-tokenizer` (★23, active Apr 2026) compiles to .so, can embed like vec0. **Deferred**: vec0 KNN semantic search compensates for unicode61's CJK weakness; coding memories are mostly English/code terms.
- [ ] **npm publish** — CI auto-publish `@agentrete/mcp` per platform.
- [ ] **sqlite-vec: pre-normalized vectors** — L2-normalize at insert time (negligible gain, skip for now).

## Done

- [x] memory_list offset pagination (list() + offset param, MCP schema + handler)
- [x] Store graceful shutdown (wal_checkpoint + pool.close() on MCP server exit)
- [x] forget cleans vec0 index (DELETE FROM vec_memories before DELETE FROM memories)
- [x] RRF fusion search (vec0 KNN + FTS5 BM25 → `1/(K+rank)` merge)

- [x] RRF fusion search (vec0 KNN + FTS5 BM25 → `1/(K+rank)` merge)
- [x] Temporal decay (`e^(-days/half_life)` score factor, configurable half_life_days)
- [x] MEMORY_PROTOCOL in MCP initialize.instructions (all 3 versions)
- [x] search_vec type filter (dynamic AND m.type=?4 when type param present)
- [x] search/list MCP output enriched with tags/importance/created_at/source_file/project
- [x] memory_search schema exposes type param
- [x] Hybrid semantic search (vec0 KNN + FTS5 BM25 + cosine rerank)
- [x] memory_compact (exact + semantic dedup)
- [x] Nested config (config-rs)
- [x] Embed worker (async batch, embed_poll_secs/embed_batch configurable)
- [x] PreToolUse hook (block sed/python3)
- [x] Detailed memory_stats (type counts, model info, DB size, vec0 status)
- [x] Seed subcommand (community rules)
- [x] bge-small-zh-v1.5 as default local model
