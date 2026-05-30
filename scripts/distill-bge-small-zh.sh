#!/bin/bash
# Distill BAAI/bge-small-zh-v1.5 to Model2Vec format
# Requires: pip install model2vec[distill]

set -euo pipefail

MODEL="${1:-BAAI/bge-small-zh-v1.5}"
OUTDIR="${2:-$HOME/.cache/model2vec/bge-small-256d}"
DIMS="${3:-256}"

echo "Distilling $MODEL → $OUTDIR ($DIMS dims)"

python3 -c "
from model2vec.distill import distill
m = distill(model_name='$MODEL', dimensions=$DIMS)
m.save_pretrained('$OUTDIR')
"

echo "Done. Model saved to $OUTDIR/"
ls -la "$OUTDIR/"
