#!/bin/sh
# agentrete Claude Code post-tool-use hook.

AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"

input=$(cat 2>/dev/null)
[ -z "$input" ] && exit 0

tool=$(echo "$input" | python3 -c "
import sys,json
try:
    d=json.load(sys.stdin)
    print(d.get('tool_name','') or d.get('tool','') or '')
except:
    print('')
" 2>/dev/null)

[ -z "$tool" ] && exit 0

case "$tool" in
    Edit|Write|Bash)
        curl -s -X POST "$AGENTRETE_URL" \
          -H "Content-Type: application/json" \
          --max-time 3 \
          -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_save\",\"arguments\":{\"content\":\"Tool call: $tool\",\"type\":\"fact\",\"tags\":\"hook,tool-call\"}}}" \
          >/dev/null 2>&1
        ;;
    *) exit 0 ;;
esac

exit 0
