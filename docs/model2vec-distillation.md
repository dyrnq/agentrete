# Model2Vec Distillation Guide

## What is Model2Vec?

Model2Vec distills a **sentence-transformers** model (BERT/RoBERTa/XLM-R encoder) into a
compact static embedding table. The distilled model performs **token embedding lookup +
weighted mean pooling** instead of running a full neural network forward pass.

Result: **10–500MB model, 0.1ms per text on CPU, zero GPU needed.**

## Compatibility

| Architecture | Distillable? | Examples |
|-------------|:---:|---------|
| BERT encoder (sentence-transformers) | ✅ | `BAAI/bge-*`, `moka-ai/m3e-*`, `shibing624/text2vec-*`, `all-MiniLM-L6-v2` |
| Decoder-only LLM | ❌ | `Qwen3-Embedding`, `LLaMA`, `GPT` — different architecture |

## bge-m3: Multilingual Flagship

**BAAI/bge-m3** is the recommended model for agentrete. XLM-RoBERTa backbone,
100+ languages, 1024d source.

Distilled at 4 dimensions via `scripts/distill-bge-m3.py`:

| dim | model.safetensors | total size | load time* | best for |
|-----|------------------|-----------|-----------|----------|
| **256d** | 128MB | **131MB** | 1.4s | low memory, fast search |
| **512d** 🏆 | 256MB | **253MB** | 1.5s | **best trade-off** |
| **768d** | 384MB | **375MB** | 2.1s | high quality |
| **1024d** | 512MB | **497MB** | 2.4s | maximum accuracy |

\* release build, `Model2Vec loaded` to server ready.

### Performance (512d, release build)

```
Embed:  10 vectors in 46ms
Search: RRF merged 3 results, <100ms
Accuracy:
  "code modification"     → apply_patch rules     ✅
  "database lock"         → SQLite WAL mode          ✅
  "修改源代码" (Chinese)  → apply_patch rules      ✅ cross-lingual
```

### Quick Start

```bash
# Distill all 4 dimensions (downloads 2.2GB once, PCA is fast)
cd /path/to/agentrete && uv run scripts/distill-bge-m3.py

# Or pick specific dims
uv run scripts/distill-bge-m3.py 512

# Then configure agentrete
cat >> ~/.agentrete/config.toml << TOML
[embedding]
backend = "model2vec"

[embedding.model2vec]
model = "BAAI/bge-m3"
model2vec_path = "~/.cache/model2vec/bge-m3-512d"
dims = 512
TOML
```

## Installation

- Python 3.12+, `uv` (recommended) or `pip`
- `uv pip install "model2vec[distill]"` or `pip install "model2vec[distill]"`

The `[distill]` extra installs `sentence-transformers`, `torch`, `tokenizers`, and `scikit-learn`.

## Distillation Script

`scripts/distill-bge-m3.py` — multi-dimension distillation with progress logging.

```bash
uv run scripts/distill-bge-m3.py          # all: 256 512 768 1024
uv run scripts/distill-bge-m3.py 512      # single dim
uv run scripts/distill-bge-m3.py 256 512  # specific dims
```

Output: `~/.cache/model2vec/bge-m3-{dim}d/` for each dimension.
Skips already-distilled dimensions.

### For other models

```python
#!/usr/bin/env python3
from model2vec.distill import distill

# bge-small-zh-v1.5 (10MB, 512d source)
m = distill(model_name="BAAI/bge-small-zh-v1.5", pca_dims=256)
m.save_pretrained("/tmp/m2v-bge-small-zh")

# m3e-base (Chinese-optimized, 768d source)
m = distill(model_name="moka-ai/m3e-base", pca_dims=256)
m.save_pretrained("/tmp/m2v-m3e-base")

# Keep original dims
m = distill(model_name="BAAI/bge-small-zh-v1.5", pca_dims=None)
m.save_pretrained("/tmp/m2v-bge-small-zh-full")
```

## How It Works

### Phase 1: Token Embedding Extraction (~20s)

1. Load the sentence-transformers model
2. Pass each token in the vocabulary through the embedding layer
3. Apply the **mean pooling weights** learned from the original model
4. Store as a static `token_id → vector` lookup table

### Phase 2: PCA Dimensionality Reduction (~10s)

PCA reduces dimensions while preserving principal components:

| Source Dims | Distilled Dims (default) |
|------------|-------------------------|
| 512 | **256** |
| 768 | **256** |
| 1024 | **256** |

**To preserve original dimensions**, pass `pca_dims=None`:

```python
m = distill(model_name="BAAI/bge-small-zh-v1.5", pca_dims=None)  # 512d
m = distill(model_name="moka-ai/m3e-base", pca_dims=None)         # 768d
```

## Model Comparison

### Chinese/Monolingual

| Model | Source Dims | Size | Speed | zh Quality | Best For |
|-------|------------|------|-------|------------|----------|
| bge-small-zh-v1.5 | 512 | **10MB** | 0.17ms | ★★★☆ | Default — smallest, fastest |
| bge-base-zh-v1.5 | 768 | ~30MB | ~0.2ms | ★★★★ | Better Chinese |
| m3e-base | 768 | ~30MB | ~0.2ms | ★★★★ | Chinese-optimized |
| text2vec-base-chinese | 768 | ~30MB | ~0.2ms | ★★★★ | MTEB-validated |

### Multilingual

| Model | Source Dims | Size | Languages | Downloads | Best For |
|-------|------------|------|-----------|-----------|----------|
| **bge-m3** 🏆 | 1024 | 131–497MB | 100+ | 30M | Flagship multilingual |
| multilingual-e5-base | 768 | ~50MB | 94 | 5.9M | E5 series |
| gte-multilingual-base | 768 | ~50MB | 75 | 1.1M | Long-context (8192) |
| paraphrase-multilingual-MiniLM-L12-v2 | 384 | ~20MB | 50+ | 49M | Smallest multilingual |

All above are sentence-transformers compatible and can be distilled.

## Troubleshooting

| Problem | Solution |
|---------|----------|
| CUDA out of memory | `os.environ["CUDA_VISIBLE_DEVICES"] = ""` |
| model.safetensors not found | Source must be sentence-transformers, not GGUF/LLM |
| Slow download | `HF_ENDPOINT=https://hf-mirror.com` or proxy |
| "encode_single" missing | Use `model.encode(["text"])[0]` in model2vec Python |

## References

- [Model2Vec paper](https://arxiv.org/abs/2501.05242)
- [model2vec-rs crate](https://crates.io/crates/model2vec-rs)
- [MinishLab/model2vec on GitHub](https://github.com/MinishLab/model2vec)
- [Compatible models on HuggingFace](https://huggingface.co/models?pipeline_tag=sentence-similarity&sort=downloads)


## Distilled Models Summary

Generated via `.dev/distill-models.py` (May 30, 2026). All models stored at `~/.cache/model2vec/{slug}-{dim}d/`.

| Model | 256d | 512d | 768d | 1024d | 384d | Notes |
|-------|------|------|------|-------|------|-------|
| **bge-small** | 10MB ✅ | 20MB ✅ | — | — | — | Default model |
| **bge-base** | 10MB ✅ | 20MB ✅ | 30MB ✅ | — | — | bge-small upgrade |
| **bge-m3** | 122MB ✅ | 244MB ✅ | 366MB ✅ | 488MB ✅ | — | Flagship multilingual |
| **m3e-small** | 10MB ✅ | 20MB ✅ | — | — | — | Chinese-optimized |
| **m3e-base** | 10MB ✅ | 20MB ✅ | 30MB ✅ | — | — | Chinese-optimized |
| **e5-multi** | 122MB ✅ | 244MB ✅ | 366MB ✅ | — | — | 94 languages |
| **text2vec** | 10MB ✅ | 20MB ✅ | 30MB ✅ | — | — | MTEB-validated |
| **minilm** | 122MB ✅ | — | — | — | 183MB ✅ | Smallest multilingual |
| **gte-multi** | ❌ | ❌ | ❌ | — | — | model2vec tokenizer bug |

### Known Issues

| Model | Reason |
|-------|--------|
| Alibaba-NLP/gte-multilingual-base | model2vec `distill()` Skeletoken `index out of bounds`. `trust_remote_code=True` doesn't help. This is an upstream model2vec bug, not a model issue. |

## Integration Test Results

All models tested with 10 seeds (5 rules + 5 patterns), 3 queries each.
Load + embed time is per-model startup including DB init.

| Model | Size | EN→EN | ZH→EN | Verdict |
|-------|------|-------|-------|---------|
| **bge-m3** 1024d | 488MB | ✅✅✅ | ✅ | 🏆 Flagship |
| **bge-m3** 512d | 244MB | ✅✅✅ | ✅ | ⭐ Best value |
| **bge-m3** 256d | 131MB | ✅✅✅ | ✅ | |
| **bge-base** 256-768d | 11-32MB | ✅✅✅ | ✅ | 🏆 Chinese |
| **bge-small** 256-512d | 11-21MB | ✅✅✅ | ✅ | 🏆 Default model |
| **m3e-base** 256-768d | 11-32MB | ✅✅✅ | ✅ | 🏆 Chinese |
| **m3e-small** 256-512d | 11-21MB | ✅✅✅ | ✅ | |
| **minilm** 256-384d | 131-192MB | ✅✅✅ | ✅ | ⚡ Smallest multilingual |
| **e5-multi** 256-768d | 131-375MB | ✅✅✅ | ❌ | EN only, no Chinese |
| **text2vec** 256-768d | 11-32MB | ✅✅✅ | ❌ | EN only, no Chinese |
| **gte-multi** | — | — | — | ❌ model2vec bug |

- **EN→EN (rule)**: "code modification apply_patch sed" → expected top result is rule type
- **EN→EN (pattern)**: "database concurrent lock" → expected top result is pattern type
- **ZH→EN**: "修改源代码用什么工具" → expected top result is rule type (cross-lingual)

### Default Model

`BAAI/bge-small-zh-v1.5` @ 256d (~10MB). Chosen for:
- Smallest footprint among full-scoring models
- Cross-lingual EN↔ZH support
- Distilled model at `~/.cache/model2vec/bge-small-256d/`

### Production Recommendation

`BAAI/bge-m3` @ 512d (~244MB). Best trade-off:
- 1024d → 512d via PCA retains strong accuracy
- 100+ language support, all 3 test queries passed
- Load time ~1.5s in release build
