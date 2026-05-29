#!/bin/sh
# agentrete prompt-submit hook — recall memories relevant to user query.

AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"

# Read stdin for prompt content (Codex passes it via hook)
input=$(cat 2>/dev/null)
if [ -z "$input" ]; then
    exit 0
fi

# Extract the first non-empty meaningful text as query
query=$(echo "$input" | python3 -c "
import sys,json
try:
    d=json.load(sys.stdin)
    prompt=d.get('prompt','') or d.get('text','') or d.get('message','')
    # Take first 120 chars as query
    print(prompt[:120])
except:
    print(sys.stdin.read()[:120])
" 2>/dev/null)

[ -z "$query" ] && exit 0
[ ${#query} -lt 3 ] && exit 0

result=$(curl -s -X POST "$AGENTRETE_URL" \
  -H "Content-Type: application/json" \
  --max-time 3 \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$query\",\"limit\":3}}}")

if echo "$result" | grep -q '"score"'; then
    echo "🧠 agentrete recall:" >&2
    echo "$result" | python3 -c "
import sys,json
r=json.load(sys.stdin)
for c in r.get('result',{}).get('content',[]):
    print(f'  {c[\"text\"][:200]}', file=sys.stderr)
" 2>/dev/null || echo "$result" >&2
fi

exit 0
