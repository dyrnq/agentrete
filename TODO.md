# Agentrete TODO

## High Priority

- [ ] **Embed Model2Vec model into binary** — `include_bytes!()` the 10MB distilled `bge-small-zh-v1.5` model → zero-config local embedding. Same pattern as `sqlite-vec` `.so`. Temp-dir extract + `StaticModel::from_bytes()` on startup.
- [ ] **Git pre-commit hook** — block commits containing `sed`/`python3` source file modifications in agentrete repo. Enforce `apply_patch`/`modify_code_block` as the only code modification methods.
- [ ] **memory_save_batch**
- [ ] **Expose `project`/`files` to MCP** — add to `memory_save` inputSchema + handler. Hook scripts pass `$PWD` basename as project. Enables cross-project memory without collision. — single MCP tool call accepting array of `{content, type, tags}` for atomic multi-INSERT. Reduces HTTP round trips.
- [ ] **bge-m3 distillation** — 完成蒸馏并集成测试。1024d, 100+ 语言，是目前可蒸馏模型中质量最高的。

## Medium Priority

- [ ] **FTS5 Chinese tokenizer** — current `unicode61` does single-char split for Chinese. Replace with `jieba` or `icu` tokenizer.
- [ ] **Search semantic dedup** — use cosine clustering in search results to merge similar memories. Already in `memory_compact` but not exposed in `memory_search`.
- [ ] **ONNX backend** — replace candle BERT remnants with ONNX Runtime (`ort` crate) for models that can't be distilled to Model2Vec (e.g., ModernBERT).
- [ ] **Embed worker configurable** — expose batch size, poll interval, max retries via config.
- [ ] **Cross-platform model2vec distiller** — bundle Python distillation as `agentrete distill` subcommand (uv + model2vec[distill]).

## Low Priority

- [ ] **npm publish** — GitHub Actions CI to auto-publish `@agentrete/mcp` npm package per platform.
- [ ] **MCP tasks support** — `tasks/*` capability for long-running operations (model download, batch embedding).
- [ ] **Pagination for memory_list** — `offset`/`cursor` support when >100 memories.
- [ ] **sqlite-vec: pre-normalized vectors** — normalize at insert time to save compute per search.
- [ ] **intfloat/multilingual-e5-base distillation** — 94 languages, 768d.
- [ ] **Alibaba-NLP/gte-multilingual-base distillation** — 75 languages, 8192 token context.
