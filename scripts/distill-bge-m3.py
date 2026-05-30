#!/usr/bin/env python3
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "model2vec[distill]",
# ]
# ///
"""Distill BAAI/bge-m3 to Model2Vec format at multiple dimensions.
Usage:
  uv run .dev/distill-bge-m3.py          # all dims: 256, 512, 768, 1024
  uv run .dev/distill-bge-m3.py 512      # single dim
  uv run .dev/distill-bge-m3.py 256 512  # specific dims
Output: ~/.cache/model2vec/bge-m3-{dim}d/
"""

import os, time, sys, logging, argparse

parser = argparse.ArgumentParser()
parser.add_argument("dims", nargs="*", type=int, default=[256, 512, 768, 1024],
                    help="PCA dimensions (default: 256 512 768 1024)")
args = parser.parse_args()

# Proxy setup
if not os.environ.get("HTTPS_PROXY"):
    os.environ["HTTPS_PROXY"] = "http://192.168.6.111:7890"
if not os.environ.get("HTTP_PROXY"):
    os.environ["HTTP_PROXY"] = "http://192.168.6.111:7890"
os.environ["HF_ENDPOINT"] = "https://huggingface.co"

from model2vec.distill import distill
import warnings
warnings.filterwarnings("ignore")
os.environ.setdefault("TQDM_DISABLE", "1")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
os.environ.setdefault("HF_HUB_DISABLE_PROGRESS_BARS", "1")
for lib in ["huggingface_hub", "transformers", "sentence_transformers", "urllib3", "requests", "urllib3.connectionpool"]:
    import logging
    logging.getLogger(lib).setLevel(logging.ERROR)
import huggingface_hub.utils.logging
huggingface_hub.utils.logging.set_verbosity_warning()
import transformers.utils.logging
transformers.utils.logging.set_verbosity_warning()

MODEL_NAME = "BAAI/bge-m3"
CACHE = os.path.expanduser("~/.cache/model2vec")

# Download once, distill at each dim
logging.basicConfig(level=logging.INFO, format="%(asctime)s %(message)s", datefmt="%H:%M:%S",
                    handlers=[logging.StreamHandler(sys.stdout),
                              logging.FileHandler("/tmp/distill-bge-m3.log")])

logging.info(f"Distilling {MODEL_NAME} at dims: {args.dims}")
logging.info("Step 1/3: Downloading model (2.2GB, ~3-5 min) ...")
t0 = time.time()

model = distill(model_name=MODEL_NAME, pca_dims=max(args.dims))

download_time = time.time() - t0
logging.info(f"Step 1 done in {download_time:.0f}s. Model has {model.dim}d.")

for dim in args.dims:
    out_dir = os.path.join(CACHE, f"bge-m3-{dim}d")
    if os.path.exists(os.path.join(out_dir, "model.safetensors")):
        logging.info(f"Skipping {dim}d — already exists at {out_dir}")
        continue

    logging.info(f"  Distilling {dim}d...")
    t = time.time()

    if dim != max(args.dims):
        # Re-distill at lower dim (PCA is cheap, no re-download)
        m = distill(model_name=MODEL_NAME, pca_dims=dim)
    else:
        m = model

    os.makedirs(out_dir, exist_ok=True)
    m.save_pretrained(out_dir)

    size_mb = sum(os.path.getsize(os.path.join(out_dir, f))
                  for f in os.listdir(out_dir) if os.path.isfile(os.path.join(out_dir, f))) / 1024 / 1024
    elapsed = time.time() - t
    logging.info(f"  {dim}d done: {size_mb:.0f}MB in {elapsed:.0f}s -> {out_dir}")

logging.info("ALL DONE")
