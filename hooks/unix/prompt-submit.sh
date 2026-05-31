#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
input=$(cat 2>/dev/null)
query=$(json_val "$input" 'prompt' "$input" | head -n1 | cut -c1-200)
[ -z "$query" ] && exit 0
result=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$query\",\"limit\":3}}}")
if echo "$result" | grep -q '"score"'; then
    echo "🧠 agentrete: relevant memories" >&2
    json_lines "$result" 'result.content' 2>/dev/null | while IFS= read -r line; do
        text=$(json_val "$line" 'text' ''); [ -n "$text" ] && echo "  $text" >&2
    done
fi
exit 0
