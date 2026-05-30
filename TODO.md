# Agentrete TODO

## High Priority

- [ ] **Model2Vec embed into binary** — `include_bytes!()` model2vec model (~50MB) into binary for zero-config local embedding. M2V_multilingual_output already downloaded. Temp-dir extract + StaticModel::from_bytes().
- [ ] **Model2Vec default model** — provide a pre-distilled model2vec model (bge-small 10MB or bge-m3 200MB). Distillation guide at docs/model2vec-distillation.md. Embed into binary or auto-download on first use.

## Medium Priority

- [ ] **FTS5 Chinese tokenizer** — `unicode61` does single-char split. Replace with `jieba` or `icu` for proper CJK search.
- [ ] **Embed worker configurable** — expose batch size, poll interval via config.
- [ ] **model2vec distiller** — `agentrete distill` subcommand (uv + model2vec[distill]).

## Low Priority

- [ ] **npm publish** — CI auto-publish `@agentrete/mcp` per platform.
- [ ] **sqlite-vec: pre-normalized vectors** — L2-normalize at insert time (negligible gain, skip for now).
- [ ] **More distillations** — multilingual-e5-base, gte-multilingual-base.

## Done

- [x] Hybrid semantic search (FTS5 + cosine rerank)
- [x] memory_compact (exact + semantic dedup)
- [x] Nested config (config-rs)
- [x] Embed worker (async batch, model-switch recompute)
- [x] PreToolUse hook (block sed/python3)
- [x] Detailed memory_stats (type counts, model info, DB size)
- [x] Seed subcommand (community rules)
- [x] bge-small-zh-v1.5 as default local model
- [x] memory_list pagination (offset)
