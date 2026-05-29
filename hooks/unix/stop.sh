#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
result=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_stats\",\"arguments\":{}}}")
count=$(json_val "$result" 'result.content[0].text' '')
echo "🧠 agentrete: session ended. $count" >&2
exit 0
