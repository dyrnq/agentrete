#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
# agentrete PreCompact hook — emergency save before context compression.
# Always fires (no interval). After compression, the AI may lose access
# to earlier details — save everything important NOW.

cat << 'MSG'
Context compression is imminent. Before the window slides, save ALL important
facts from this session to memory:

1. Decisions made (with rationale) — type "decision"
2. Bugs found and fixed (with root cause) — type "bug"
3. Architecture choices (with alternatives considered) — type "decision"
4. Non-obvious gotchas or constraints discovered — type "fact"

For each: call memory_save with content, type, and tags.

Prioritize decisions over narrative. Cite file paths where possible using source_file.
After saving, respond "saved N memories" so the user can see what was persisted.
MSG

exit 2
