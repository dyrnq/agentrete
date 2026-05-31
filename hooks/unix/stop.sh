#!/bin/sh
# agentrete Stop hook — interval-based save reminder.
# Fires every 10th session stop. Reminds AI to save key decisions.
# Philosophy: the hook is a safety net. AI should proactively save
# via MEMORY_PROTOCOL. This hook catches AIs that forget.

set -eu

INTERVAL="${AGENTRETE_SAVE_INTERVAL:-10}"
COUNTER_FILE="${HOME}/.agentrete/.stop_counter"

mkdir -p "$(dirname "$COUNTER_FILE")"
count=$(cat "$COUNTER_FILE" 2>/dev/null || echo 0)
count=$((count + 1))
echo "$count" > "$COUNTER_FILE"

# Only block every Nth stop
if [ $((count % INTERVAL)) -ne 0 ]; then
    exit 0
fi

cat << 'MSG'
Before stopping, save the key decisions from this conversation to memory.

For each decision:
1. Describe the decision + its rationale (not just "we used X", but "we used X because Y")
2. Call memory_save with:
   - content: full decision + rationale
   - type: "decision"
   - tags: relevant keywords

If no new decisions were made, respond "nothing to save" and stop.
MSG

exit 2
