#!/usr/bin/env bash
# agentrete smoke test — cross-platform (Linux / macOS / Windows Git Bash)
# Usage: ./scripts/smoke-test.sh <binary_path> [port]
set -euo pipefail

BIN="${1:-./target/debug/agentrete}"
PORT="${2:-19099}"
CONFIG_DIR="/tmp/agentrete-smoke-$$"

cleanup() {
    kill "$PID" 2>/dev/null || true
    rm -rf "$CONFIG_DIR"
}
trap cleanup EXIT

mkdir -p "$CONFIG_DIR"

printf 'port = %s\n' "$PORT" > "$CONFIG_DIR/config.toml"
printf 'db_dir = "%s"\n' "$CONFIG_DIR" >> "$CONFIG_DIR/config.toml"
cat >> "$CONFIG_DIR/config.toml" << 'TOML'

[embedding]
backend = "none"

[knowledge_graph]
enabled = false
TOML

echo "=== Starting agentrete ==="
"$BIN" -c "$CONFIG_DIR/config.toml" mcp --port "$PORT" &
PID=$!

for i in $(seq 1 20); do
    if curl -s "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then
        echo "Ready after ${i}s"
        break
    fi
    sleep 1
done

# Health check
HEALTH=$(curl -s "http://127.0.0.1:$PORT/")
echo "$HEALTH" | grep -q '"status":"ok"' || { echo "FAIL: health"; exit 1; }
echo "PASS: health"

# Save
SAVE=$(curl -s -X POST "http://127.0.0.1:$PORT/" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"ci smoke test","type":"test"}}}')
echo "$SAVE" | grep -q "Saved:" || { echo "FAIL: save — $SAVE"; exit 1; }
echo "PASS: save"

# Stats
STATS=$(curl -s -X POST "http://127.0.0.1:$PORT/" \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_stats","arguments":{}}}')
echo "$STATS" | grep -q "Memories: 1" || { echo "FAIL: stats — $STATS"; exit 1; }
echo "PASS: stats"

echo "=== Smoke test PASSED ==="
