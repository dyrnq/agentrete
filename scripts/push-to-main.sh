#!/bin/bash
# Push current tree to origin/main as a single orphan commit.
# Local sqlite branch history is preserved.
# Usage: ./scripts/push-to-main.sh ["commit message"]
set -e
MSG="${1:-feat: agentrete update}"
SQUASHED=$(git commit-tree HEAD^{tree} -m "$MSG")
git push origin "$SQUASHED:refs/heads/main" --force
echo "Pushed: ${SQUASHED:0:7} → origin/main"
