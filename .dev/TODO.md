# Agentrete TODO

## High Priority

- [ ] **memory_save_batch** — single MCP tool call accepting array of `{content, type, tags}` for atomic multi-INSERT
- [ ] **Git pre-commit hook** — block commits containing `sed`/`python3` source file modifications. Enforce `apply_patch` as the only code modification method.
- [ ] **Embed Model2Vec model into binary** — `include_bytes!()` the 10MB distilled model → zero-config local embedding, same pattern as sqlite-vec extension
- [ ] **bge-m3 distillation** — 1024d, 100+ languages. Highest quality distillable model available.

## Medium Priority

- [ ] **FTS5 Chinese tokenizer** — current `unicode61` does single-char split for Chinese. Replace with `jieba` or `icu` tokenizer.
- [ ] **Search semantic dedup** — cosine clustering in search results to merge similar memories. Logic exists in `memory_compact` but not exposed in `memory_search`.
- [ ] **CLI → HTTP API** — CLI commands currently operate directly on DB, conflicting with running MCP server. Should POST to MCP server instead.
- [ ] **memory_list pagination** — `offset`/`cursor` support exposed via MCP parameters for >100 memories.
- [ ] **ONNX backend** — ONNX Runtime (`ort` crate) for models that can't be distilled to Model2Vec (e.g., ModernBERT).
- [ ] **Cross-platform model2vec distiller** — bundle Python distillation as `agentrete distill` subcommand.

## Low Priority

- [ ] **npm publish** — GitHub Actions CI to auto-publish `@agentrete/mcp` npm package per platform.
- [ ] **sqlite-vec: pre-normalized vectors** — normalize at insert time to save compute per search.
- [ ] **intfloat/multilingual-e5-base distillation** — 94 languages, 768d.
- [ ] **Alibaba-NLP/gte-multilingual-base distillation** — 75 languages, 8192 token context.
- [ ] **记忆导出/导入** — `agentrete export/import` 子命令, JSON/CSV 格式.
- [ ] **Web Dashboard** — graphical memory browser, similar to agentmemory viewer.
