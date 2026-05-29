# Model2Vec Distillation Guide

## What is Model2Vec?

Model2Vec distills a **sentence-transformers** model (BERT/RoBERTa/XLM-R encoder) into a
compact static embedding table. The distilled model performs **token embedding lookup +
weighted mean pooling** instead of running a full neural network forward pass.

Result: **10–400MB model, 0.1ms per text on CPU, zero GPU needed.**

## Compatibility

| Architecture | Distillable? | Examples |
|-------------|:---:|---------|
| BERT encoder (sentence-transformers) | ✅ | `BAAI/bge-*-zh-v*`, `moka-ai/m3e-*`, `shibing624/text2vec-*`, `all-MiniLM-L6-v2` |
| Decoder-only LLM | ❌ | `Qwen3-Embedding`, `LLaMA`, `GPT` — different architecture, can't extract static embeddings |

## Installation

### Prerequisites

- Python 3.10+
- `uv` (recommended) or `pip`

### Setup

```bash
# Using uv (recommended — faster, isolated)
uv venv /tmp/m2v-distill
source /tmp/m2v-distill/bin/activate  # Linux/macOS
# or: /tmp/m2v-distill\Scripts\activate  # Windows

uv pip install "model2vec[distill]"
```

Or with pip:

```bash
pip install "model2vec[distill]"
```

The `[distill]` extra installs `sentence-transformers`, `torch`, `tokenizers`, and `scikit-learn`.

## Distillation Script

Save as `distill_model.py`:

```python
#!/usr/bin/env python3
"""Distill a sentence-transformers model to Model2Vec format."""

import sys, time, os
from model2vec.distill import distill

MODELS = {
    # name          -> output path          expected dims
    "bge-small":    ("BAAI/bge-small-zh-v1.5",    "/tmp/m2v-bge-small-zh",    512),
    "bge-base":     ("BAAI/bge-base-zh-v1.5",     "/tmp/m2v-bge-base-zh",     768),
    "m3e-base":     ("moka-ai/m3e-base",          "/tmp/m2v-m3e-base",        768),
    "text2vec":     ("shibing624/text2vec-base-chinese", "/tmp/m2v-text2vec",  768),
}

def distill_model(name: str):
    if name not in MODELS:
        print(f"Unknown model: {name}")
        print(f"Available: {', '.join(MODELS.keys())}")
        sys.exit(1)

    model_id, out_dir, expected_dims = MODELS[name]

    print(f"Distilling {model_id} → {out_dir} ...")
    t = time.time()

    # Distill (downloads model from HuggingFace if not cached)
    m = distill(model_name=model_id)  # defaults: PCA to 256d
    # m = distill(model_name=model_id, pca_dims=None)  # keep original dims
    print(f"  Distilled in {time.time() - t:.1f}s")

    # Save
    os.makedirs(out_dir, exist_ok=True)
    m.save_pretrained(out_dir)

    # Verify
    emb = m.encode_single("测试中文嵌入向量质量")
    print(f"  Dimensions: {len(emb)} (expected: {expected_dims})")

    # Speed test
    t = time.time()
    _ = m.encode(["benchmark test sentence"] * 100)
    print(f"  100 texts: {(time.time() - t) * 1000:.1f}ms ({((time.time() - t) / 100) * 1000:.2f}ms each)")

    # Quality test
    emb_zh = m.encode_single("不要用sed修改代码")
    emb_en = m.encode_single("Never use sed to modify source code")
    emb_noise = m.encode_single("今天天气真好适合出去玩")
    cos = lambda a, b: sum(x*y for x,y in zip(a,b)) / (sum(x*x for x in a)**0.5 * sum(y*y for y in b)**0.5 + 1e-10)
    print(f"  Cosine: zh↔en={cos(emb_zh, emb_en):.4f}  zh↔noise={cos(emb_zh, emb_noise):.4f}")

    # File sizes
    total = sum(
        os.path.getsize(os.path.join(out_dir, f))
        for f in os.listdir(out_dir)
        if os.path.isfile(os.path.join(out_dir, f))
    )
    print(f"  Model size: {total / 1024 / 1024:.1f}MB")
    print(f"  Saved to: {out_dir}")
    print()

if __name__ == "__main__":
    name = sys.argv[1] if len(sys.argv) > 1 else "bge-small"
    distill_model(name)
```

## Usage

```bash
# Distill bge-small (default, 512d, ~10MB)
python3 distill_model.py bge-small

# Distill bge-base (768d, larger)
python3 distill_model.py bge-base

# Distill m3e-base (768d, Chinese-optimized)
python3 distill_model.py m3e-base
```

## How It Works

Model2Vec distillation has two phases:

### Phase 1: Token Embedding Extraction (~20s)

1. Load the sentence-transformers model
2. Pass each token in the vocabulary through the embedding layer
3. Apply the **mean pooling weights** learned from the original model
4. Store as a static `token_id → vector` lookup table

### Phase 2: PCA Dimensionality Reduction (~10s)

By default, Model2Vec applies PCA to reduce dimensions while preserving the principal
components of the token embedding space. The default behavior:

| Source Dims | Distilled Dims (default) |
|------------|-------------------------|
| 512 | **256** |
| 768 | **256** |
| 1024 | **256** |

**To preserve original dimensions**, pass `pca_dims=None`:

```python
from model2vec.distill import distill

# Keep 512d from bge-small source
m = distill(model_name="BAAI/bge-small-zh-v1.5", pca_dims=None)
# → 512d output

# Keep 768d from m3e-base source
m = distill(model_name="moka-ai/m3e-base", pca_dims=None)
# → 768d output
```

**Trade-offs**:

| Approach | Model Size | Speed | Accuracy |
|----------|-----------|-------|----------|
| PCA to 256d (default) | Smallest (~10MB) | Fastest (0.17ms) | Good — retains ~95% variance |
| Keep original dims | Larger (~25–50MB) | Slightly slower | Better — no information loss |
| Custom `pca_dims=N` | Proportional | Proportional | Tunable |

The result is a single `.safetensors` file (token embedding table) + `tokenizer.json` + `config.json`.

## Model Comparison

### Chinese/Monolingual Models

| Model | Source Dims | Size | Speed | zh Quality | Best For |
|-------|------------|------|-------|------------|----------|
| **bge-small-zh-v1.5** | 512 | **10MB** | 0.17ms | ★★★☆ | Default — smallest, fastest |
| bge-base-zh-v1.5 | 768 | ~30MB | ~0.2ms | ★★★★ | Better Chinese semantics |
| m3e-base | 768 | ~30MB | ~0.2ms | ★★★★ | Chinese-optimized (moka) |
| text2vec-base-chinese | 768 | ~30MB | ~0.2ms | ★★★★ | Chinese, MTEB-validated |

### Multilingual Models (Cross-lingual)

| Model | Source Dims | Size | Speed | Languages | Downloads | Best For |
|-------|------------|------|-------|-----------|-----------|----------|
| **BAAI/bge-m3** 🏆 | 1024 | ~200MB | ~0.3ms | **100+** | 30M | Flagship multilingual (XLM-RoBERTa) |
| intfloat/multilingual-e5-base | 768 | ~50MB | ~0.2ms | 94 | 5.9M | E5 series, strong cross-lingual |
| Alibaba-NLP/gte-multilingual-base | 768 | ~50MB | ~0.2ms | 75 | 1.1M | Alibaba mGTE, long-context (8192 tokens) |
| sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2 | 384 | ~20MB | ~0.15ms | 50+ | 49M | Smallest multilingual |

> **Note**: All models above are **sentence-transformers compatible** and can be
> distilled with Model2Vec.
>
> **Default in agentrete**: `bge-small-zh-v1.5` (10MB, 256d).
> With `include_bytes!()` embedding planned, users won't need to run distillation at all.
>
> **Recommending `bge-m3`** for production multilingual use — 1024d vectors,
> 100+ languages, best cross-lingual accuracy among distillable models.

## Troubleshooting

### "CUDA out of memory"

Distillation runs on CPU by default. If your system forces CUDA:

```python
import os
os.environ["CUDA_VISIBLE_DEVICES"] = ""
from model2vec.distill import distill
m = distill(model_name="BAAI/bge-small-zh-v1.5")
```

### "model.safetensors not found"

The source model must be a sentence-transformers model with `model.safetensors` file.
Models that only have `pytorch_model.bin` or are GGUF format cannot be distilled.

### Slow download

Set HuggingFace mirror:

```bash
export HF_ENDPOINT=https://hf-mirror.com
python3 distill_model.py
```

Or use proxy:

```bash
HTTPS_PROXY=http://proxy:7890 python3 distill_model.py
```

## References

- [Model2Vec paper](https://arxiv.org/abs/2501.05242)
- [model2vec-rs crate](https://crates.io/crates/model2vec-rs)
- [MinishLab/model2vec on GitHub](https://github.com/MinishLab/model2vec)
- [Compatible models on HuggingFace](https://huggingface.co/models?pipeline_tag=sentence-similarity&sort=downloads)
