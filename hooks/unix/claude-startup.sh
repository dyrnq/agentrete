#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
PROJECT=$(git rev-parse --show-toplevel 2>/dev/null | xargs basename 2>/dev/null || basename "$PWD")
result=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$PROJECT\",\"limit\":5}}}")
if echo "$result" | grep -q '"score"'; then
    echo "🧠 agentrete: project context for $PROJECT" >&2
    json_lines "$result" 'result.content' 2>/dev/null | while IFS= read -r line; do
        text=$(json_val "$line" 'text' ''); [ -n "$text" ] && echo "  $text" >&2
    done
fi
exit 0
