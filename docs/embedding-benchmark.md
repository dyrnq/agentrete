# Embedding Model Benchmark

(2026-05)

Benchmarked on Ollama server with 5 Chinese/English mixed texts (zh_rule, en_rule, zh_build, en_build, zh_noise, en_noise).

| Model | Dims | Speed | Cross-Lingual | Noise Rejection | Verdict |
|-------|------|-------|---------------|-----------------|---------|
| **granite-embedding:278m** | 768 | 0.1s | 0.77 | 0.48/0.40 | **Default** — balanced |
| qwen3-embedding | 4096 | 0.1s | **0.84** | 0.42/0.32 | Best cross-lingual, poor noise rejection |
| nomic-embed-text-v2-moe | 768 | 1.6s | 0.81 | **0.08/0.06** | Best noise rejection, weak semantics (en_build↔zh_build=0.23) |
| nomic-embed-text | 768 | 0.1s | 0.47 | 0.33 | Poor cross-lingual |
| mxbai-embed-large | 1024 | 0.1s | 0.51 | 0.55 | Poor cross-lingual |

**Cross-Lingual**: cosine similarity between Chinese and English versions of the same rule. Higher is better.  
**Noise Rejection**: cosine similarity between a coding rule and an irrelevant sentence ("what to eat tonight"). Lower is better.

### Recommendation

- **Memory/speed sensitive**: `granite-embedding:278m` (278MB, 768d)
- **Accuracy over all else**: `qwen3-embedding:latest` (7.6B, 4096d) — but tune the similarity threshold
- **Need to filter noise aggressively**: `nomic-embed-text-v2-moe` (768d) — but loses semantic nuance
