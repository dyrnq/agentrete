#!/bin/sh
# agentrete subagent-stop hook — save subagent observations.
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"

curl -s -X POST "$AGENTRETE_URL" \
  -H "Content-Type: application/json" \
  --max-time 3 \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"Subagent completed","type":"fact","tags":"hook,subagent"}}}' \
  >/dev/null 2>&1

exit 0
