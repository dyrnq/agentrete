#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
input=$(cat 2>/dev/null)
agent=$(json_val "$input" 'subagent_type' 'unknown')
result=$(mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_save\",\"arguments\":{\"content\":\"Subagent $agent started\",\"type\":\"fact\",\"tags\":\"subagent\"}}}" 2>/dev/null)
echo "🧠 agentrete: recorded subagent $agent" >&2
exit 0
