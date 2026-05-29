#!/bin/sh
# agentrete Claude Code startup hook.

AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
PROJECT=$(git rev-parse --show-toplevel 2>/dev/null | xargs basename 2>/dev/null || basename "$PWD")
CACHE="/tmp/agentrete-claude-$(echo "$PWD" | md5sum | cut -c1-8 2>/dev/null || echo "default").cache"

[ -f "$CACHE" ] && exit 0

result=$(curl -s -X POST "$AGENTRETE_URL" \
  -H "Content-Type: application/json" \
  --max-time 3 \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$PROJECT\",\"limit\":5}}}")

if echo "$result" | grep -q '"score"'; then
    touch "$CACHE"
    echo "🧠 agentrete: project memories for $PROJECT" >&2
    echo "$result" | python3 -c "
import sys,json
r=json.load(sys.stdin)
for c in r.get('result',{}).get('content',[]):
    print(f'  {c[\"text\"]}', file=sys.stderr)
" 2>/dev/null
fi

touch "$CACHE"
exit 2
