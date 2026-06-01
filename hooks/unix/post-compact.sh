#!/bin/sh
# agentrete PostCompact hook — reload context after compaction.
# Shows memory stats + searches project memories to rebuild context.
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
PROJECT=$(detect_project)

# Show memory stats
stats=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_stats\",\"arguments\":{}}}")
count=$(json_val "$stats" 'result.content[0].text' '')
echo "🧠 agentrete: $count" >&2

# Reload project context
result=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$PROJECT\",\"limit\":5}}}")
if echo "$result" | grep -q '"score"'; then
    echo "🧠 project context for $PROJECT" >&2
    json_lines "$result" 'result.content' 2>/dev/null | while IFS= read -r line; do
        text=$(json_val "$line" 'text' ''); [ -n "$text" ] && echo "  $text" >&2
    done
fi
exit 0
