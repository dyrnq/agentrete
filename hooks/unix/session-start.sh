#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
PROJECT=$(git rev-parse --show-toplevel 2>/dev/null | xargs basename 2>/dev/null || basename "$PWD")
CACHE="/tmp/agentrete-startup-$(echo "$PWD" | md5sum | cut -c1-8 2>/dev/null || echo "default").cache"
if [ -f "$CACHE" ]; then
    cache_age=$(($(date +%s) - $(stat -c%Y "$CACHE" 2>/dev/null || stat -f%m "$CACHE" 2>/dev/null || echo 0)))
    [ "$cache_age" -lt 3600 ] && exit 0
fi
result=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$PROJECT\",\"limit\":5}}}")
if echo "$result" | grep -q '"score"'; then
    touch "$CACHE"
    echo "🧠 agentrete: project memories for $PROJECT" >&2
    json_lines "$result" 'result.content' 2>/dev/null | while IFS= read -r line; do
        text=$(json_val "$line" 'text' ''); [ -n "$text" ] && echo "  $text" >&2
    done
fi
touch "$CACHE"; exit 0
