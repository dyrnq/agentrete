#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
input=$(cat 2>/dev/null)
tool=$(json_val "$input" 'tool_name' '')
case "$tool" in
    Edit|Write|apply_patch|Bash)
        content="Used $tool to write code"
        ;;
    *) exit 0 ;;
esac
mcp_post "$AGENTRETE_URL" "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_save\",\"arguments\":{\"content\":\"$content\",\"type\":\"fact\",\"tags\":\"hook-auto\"}}}" >/dev/null 2>&1
exit 0
