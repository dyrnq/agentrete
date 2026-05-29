# Embedding Model Benchmark

Benchmarked on 6 Chinese/English mixed texts (zh_rule, en_rule, zh_build, en_build, zh_noise, en_noise).

## Comparison

| Model | Backend | Dims | Size | Speed | Cross-Lingual ↑ | Noise Reject ↓ | Verdict |
|-------|---------|------|------|-------|-----------------|----------------|---------|
| **m3e-base** | local (candle) | 768 | 391MB | ~3s load / 50ms | ~0.82 | ~0.35 | Good Chinese, no GPU needed |
| **granite-embedding:278m** | remote (Ollama) | 768 | 278MB | 0.1s | 0.77 | 0.48/0.42 | **Default** — balanced |
| qwen3-embedding | remote (Ollama) | 4096 | 7.6B | 0.2s | **0.84** | 0.42/0.38 | Best cross-lingual, poor noise rejection |
| nomic-embed-text-v2-moe | remote (Ollama) | 768 | ~1GB | 0.1s | 0.81 | **0.08/0.06** | Best noise rejection, weak semantics |
| nomic-embed-text | remote (Ollama) | 768 | 137MB | 0.9s | 0.75 | 0.50/0.38 | Poor cross-lingual, slow |
| mxbai-embed-large | remote (Ollama) | 1024 | 600MB | 0.9s | 0.71 | 0.59/0.31 | Poor Chinese cross-lingual |

**Cross-Lingual**: cosine similarity between Chinese and English versions of the same rule. Higher is better.  
**Noise Reject**: cosine similarity between a coding rule and an irrelevant sentence. Lower is better (shown as zh_noise/en_noise).

## Per-Model Notes

### m3e-base (local)
- **Pros**: No network, offline, 391MB disk. Good Chinese semantics. Fast after initial load.
- **Cons**: 3s startup to load into memory. candle CPU inference only (no GPU path currently). 
- **Best for**: Air-gapped environments, minimal dependencies.

### granite-embedding:278m (remote)
- **Pros**: 278MB, fast, balanced cross-lingual (0.77) and noise rejection (0.48). IBM's multilingual model.
- **Cons**: Requires Ollama server. Noise rejection could be better.
- **Best for**: General purpose, default choice.

### qwen3-embedding (remote)
- **Pros**: Best cross-lingual (0.84). 100+ languages. 4096d high-resolution vectors. Alibaba's flagship.
- **Cons**: 7.6B params, needs GPU. Poor noise rejection — unrelated content scores 0.42.
- **Best for**: When semantic accuracy matters more than noise filtering.

### nomic-embed-text-v2-moe (remote)
- **Pros**: Outstanding noise rejection (0.08). Mixture-of-Experts architecture (Matryoshka).
- **Cons**: Weak semantic binding — "代码修改检查" ↔ "cargo build" scores only 0.23.
- **Best for**: When you have lots of noise and need aggressive filtering.

## Recommendation

| Scenario | Model |
|----------|-------|
| No network / air-gapped | `m3e-base` (local) |
| Balanced (default) | `granite-embedding:278m` |
| Best accuracy, have GPU | `qwen3-embedding:latest` |
| Lots of noise to filter | `nomic-embed-text-v2-moe` |
