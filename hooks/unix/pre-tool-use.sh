#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
input=$(cat 2>/dev/null)
tool=$(json_val "$input" 'tool_name' '')
[ "$tool" != "Bash" ] && exit 0
cmd=$(json_val "$input" 'tool_input.command' '')
echo "🔧 agentrete: about to run: $cmd" >&2
exit 0
