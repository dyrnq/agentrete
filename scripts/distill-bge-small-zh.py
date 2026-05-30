#!/usr/bin/env python3
"""Distill BAAI/bge-small-zh-v1.5 to Model2Vec format.

Usage:
    python3 scripts/distill-bge-small-zh.py [MODEL] [OUTDIR] [DIMS]

Defaults:
    MODEL  = BAAI/bge-small-zh-v1.5
    OUTDIR = ~/.cache/model2vec/bge-small-256d
    DIMS   = 256

Requires: pip install model2vec[distill]
"""

import os
import sys

MODEL = sys.argv[1] if len(sys.argv) > 1 else "BAAI/bge-small-zh-v1.5"
OUTDIR = sys.argv[2] if len(sys.argv) > 2 else os.path.expanduser("~/.cache/model2vec/bge-small-256d")
DIMS = int(sys.argv[3]) if len(sys.argv) > 3 else 256

print(f"Distilling {MODEL} → {OUTDIR} ({DIMS} dims)")

from model2vec.distill import distill

m = distill(model_name=MODEL, dimensions=DIMS)
m.save_pretrained(OUTDIR)

print(f"Done. Model saved to {OUTDIR}/")
for f in sorted(os.listdir(OUTDIR)):
    path = os.path.join(OUTDIR, f)
    print(f"  {f}  {os.path.getsize(path):>10,} bytes")
