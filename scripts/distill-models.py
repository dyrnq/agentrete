#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "model2vec[distill]",
# ]
# ///
"""Distill sentence-transformers models to Model2Vec at multiple dimensions.
Usage:
  uv run .dev/distill-models.py                    # all models, all dims
  uv run .dev/distill-models.py gte-multi          # single model
  uv run .dev/distill-models.py gte-multi 256 512  # model + specific dims
  uv run .dev/distill-models.py -E bge-m3          # exclude model(s)
  uv run .dev/distill-models.py -E bge-m3 minilm   # exclude multiple

Models:
  bge-small     BAAI/bge-small-zh-v1.5                 512  → 256, 512       (~10MB)
  bge-base      BAAI/bge-base-zh-v1.5                  768  → 256, 512, 768  (~30MB)
  bge-m3        BAAI/bge-m3                            1024 → 256,512,768,1024 (131-497MB)
  m3e-small     moka-ai/m3e-small                      512  → 256, 512       (~10MB)
  m3e-base      moka-ai/m3e-base                       768  → 256, 512, 768  (~30MB)
  gte-multi     Alibaba-NLP/gte-multilingual-base      768  → 256, 512, 768  (~50MB, 8192ctx)
  e5-multi      intfloat/multilingual-e5-base          768  → 256, 512, 768  (~50MB, 94lang)
  text2vec      shibing624/text2vec-base-chinese       768  → 256, 512, 768  (~30MB, MTEB)
  minilm        paraphrase-multilingual-MiniLM-L12-v2  384  → 256, 384       (~20MB, tiny)

Output: ~/.cache/model2vec/{slug}-{dim}d/
"""

import os, time, sys, argparse

# ─── MUST be first: suppress ALL library logging before any import ───────────
os.environ.setdefault("HF_HUB_VERBOSITY", "error")
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
os.environ.setdefault("TQDM_DISABLE", "1")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
os.environ.setdefault("TRANSFORMERS_VERBOSITY", "error")
os.environ.setdefault("CUDA_VISIBLE_DEVICES", "")

MODELS = {
    "bge-small":  ("BAAI/bge-small-zh-v1.5",                       [256, 512],           512),
    "bge-base":   ("BAAI/bge-base-zh-v1.5",                       [256, 512, 768],      768),
    "bge-m3":     ("BAAI/bge-m3",                                  [256, 512, 768, 1024], 1024),
    "m3e-small":  ("moka-ai/m3e-small",                            [256, 512],           512),
    "m3e-base":   ("moka-ai/m3e-base",                            [256, 512, 768],      768),
    "gte-multi":  ("Alibaba-NLP/gte-multilingual-base",            [256, 512, 768],      768),
    "e5-multi":   ("intfloat/multilingual-e5-base",                [256, 512, 768],      768),
    "text2vec":   ("shibing624/text2vec-base-chinese",            [256, 512, 768],      768),
    "minilm":     ("sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2", [256, 384], 384),
}

parser = argparse.ArgumentParser()
parser.add_argument("model", nargs="?", choices=MODELS.keys(),
                    help="Model slug to distill (default: all)")
parser.add_argument("dims", nargs="*", type=int,
                    help="PCA dimensions (default: model's defaults)")
parser.add_argument("-E", "--exclude", nargs="*", choices=MODELS.keys(),
                    help="Model slugs to skip")
parser.add_argument("-v", "--verbose", action="store_true",
                    help="Show HTTP request logs and library output")
args = parser.parse_args()

# HuggingFace token for higher rate limits
# Set HF_TOKEN env var if you have a HuggingFace token for higher rate limits

# Uncomment to set proxy if needed:
# os.environ.setdefault("HTTPS_PROXY", "http://proxy:7890")
# os.environ.setdefault("HTTP_PROXY", "http://proxy:7890")


from model2vec.distill import distill
from tqdm import tqdm
# Use a muted tqdm that only logs completion
_original_tqdm = tqdm
class _QuietTqdm:
    def __init__(self, *a, **kw):
        kw.setdefault("disable", True)
        self._t = _original_tqdm(*a, **kw)
    def __getattr__(self, name):
        return getattr(self._t, name)
# Patch: transformers uses tqdm.auto which we can't easily override.
# Instead, just set env var to suppress
os.environ.setdefault("TQDM_DISABLE", "1")

# Silence ALL verbose output — only keep distill progress
import warnings
warnings.filterwarnings("ignore")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
os.environ.setdefault("TQDM_DISABLE", "1")
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
# Suppress all library logging
for lib in ["huggingface_hub", "transformers", "sentence_transformers", "urllib3", "requests", "urllib3.connectionpool"]:
    import logging
    logging.getLogger(lib).setLevel(logging.ERROR)

CACHE = os.path.expanduser("~/.cache/model2vec")

import logging
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S",
                    handlers=[logging.StreamHandler(sys.stdout)])

targets = [args.model] if args.model else list(MODELS.keys())
if args.exclude:
    targets = [t for t in targets if t not in args.exclude]

results = {"ok": {}, "skip": {}, "fail": {}}

for slug in targets:
    model_name, default_dims, source_dim = MODELS[slug]
    dims = args.dims if args.dims else default_dims

    logging.info(f"=== {slug}: {model_name} ({source_dim}d source) ===")
    ok_dims, skip_dims, fail_dims = [], [], []

    try:
        # gte-multi has custom CUDA code; force CPU
        if slug == "gte-multi":
            os.environ["CUDA_VISIBLE_DEVICES"] = ""
        
        t0 = time.time()
        model = distill(model_name=model_name, pca_dims=max(dims), trust_remote_code=True)
        download_time = time.time() - t0
        logging.info(f"  Downloaded + base distill in {download_time:.0f}s ({model.dim}d)")

        for dim in dims:
            out_dir = os.path.join(CACHE, f"{slug}-{dim}d")
            if os.path.exists(os.path.join(out_dir, "model.safetensors")):
                logging.info(f"  {dim}d: SKIP (exists)")
                skip_dims.append(str(dim))
                continue

            try:
                t = time.time()
                if dim != max(dims):
                    m = distill(model_name=model_name, pca_dims=dim, trust_remote_code=True)
                else:
                    m = model

                os.makedirs(out_dir, exist_ok=True)
                m.save_pretrained(out_dir)

                size_mb = sum(os.path.getsize(os.path.join(out_dir, f))
                              for f in os.listdir(out_dir) if os.path.isfile(os.path.join(out_dir, f))) / 1024 / 1024
                elapsed = time.time() - t
                logging.info(f"  {dim}d: {size_mb:.0f}MB in {elapsed:.0f}s -> {out_dir}")
                ok_dims.append(str(dim))
            except Exception as e:
                logging.error(f"  {dim}d: FAILED — {e}")
                fail_dims.append(f"{dim}d ({e})")

        del model
    except Exception as e:
        logging.error(f"  FAILED to download/distill — {e}")
        fail_dims = [f"load failed ({e})"]
        skip_dims = []

    if ok_dims:
        results["ok"][slug] = ok_dims
    if skip_dims:
        results["skip"][slug] = skip_dims
    if fail_dims:
        results["fail"][slug] = fail_dims

# ─── Summary report ──────────────────────────────────────────────────────────
logging.info("")
logging.info("=" * 60)
logging.info("DISTILLATION SUMMARY")
logging.info("=" * 60)

total_ok = sum(len(v) for v in results["ok"].values())
total_skip = sum(len(v) for v in results["skip"].values())
total_fail = sum(len(v) for v in results["fail"].values())

if results["ok"]:
    logging.info(f"SUCCESS ({total_ok}):")
    for slug, dims in sorted(results["ok"].items()):
        model_name = MODELS[slug][0]
        logging.info(f"  {slug:12s} {', '.join(d+'d' for d in dims):20s} -> {CACHE}/{slug}-*d/")

if results["skip"]:
    logging.info(f"SKIPPED ({total_skip}):")
    for slug, dims in sorted(results["skip"].items()):
        logging.info(f"  {slug:12s} {', '.join(d+'d' for d in dims)} (already exists)")

if results["fail"]:
    logging.info(f"FAILED ({total_fail}):")
    for slug, reasons in sorted(results["fail"].items()):
        for reason in reasons:
            logging.info(f"  {slug:12s} {reason}")

logging.info(f"TOTAL: {total_ok} ok, {total_skip} skip, {total_fail} fail")
logging.info("=" * 60)
