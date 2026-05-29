#!/bin/sh
# agentrete session-start hook — load project context on new session.

AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"
PROJECT=$(git rev-parse --show-toplevel 2>/dev/null | xargs basename 2>/dev/null || basename "$PWD")
CACHE="/tmp/agentrete-startup-$(echo "$PWD" | md5sum | cut -c1-8 2>/dev/null || echo "default").cache"

# Only run once per hour per project
if [ -f "$CACHE" ]; then
    cache_age=$(($(date +%s) - $(stat -c%Y "$CACHE" 2>/dev/null || stat -f%m "$CACHE" 2>/dev/null || echo 0)))
    [ "$cache_age" -lt 3600 ] && exit 0
fi

# Search for project-related memories
result=$(curl -s -X POST "$AGENTRETE_URL" \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"memory_search\",\"arguments\":{\"query\":\"$PROJECT\",\"limit\":5}}}")

# If we got results, output to stderr so Codex sees them
if echo "$result" | grep -q '"score"'; then
    touch "$CACHE"
    echo "🧠 agentrete: project memories for $PROJECT" >&2
    echo "$result" | python3 -c "
import sys,json
r=json.load(sys.stdin)
for c in r.get('result',{}).get('content',[]):
    print(f\"  {c['text']}\", file=sys.stderr)
" 2>/dev/null || echo "$result" >&2
fi

touch "$CACHE"
exit 0
