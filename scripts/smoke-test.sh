#!/usr/bin/env bash
# agentrete smoke test — Linux / macOS / Windows (Git Bash)
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

# ── HTTP helper (curl on unix, powershell on windows) ──────────────────────
http_get() {
    if command -v curl >/dev/null 2>&1; then
        curl -s "$1"
    else
        powershell -NoProfile -Command "Invoke-RestMethod -Uri '$1' -TimeoutSec 5 | ConvertTo-Json -Depth 4 -Compress"
    fi
}

http_post() {
    if command -v curl >/dev/null 2>&1; then
        curl -s -X POST "$1" -H "Content-Type: application/json" -d "$2"
    else
        powershell -NoProfile -Command "Invoke-RestMethod -Uri '$1' -Method Post -Body '$2' -ContentType 'application/json' -TimeoutSec 5 | ConvertTo-Json -Depth 4 -Compress"
    fi
}

# ── Config ──────────────────────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR"
printf 'port = %s\n' "$PORT" > "$CONFIG_DIR/config.toml"
printf 'db_dir = "%s"\n' "$CONFIG_DIR" >> "$CONFIG_DIR/config.toml"
cat >> "$CONFIG_DIR/config.toml" << 'TOML'

[embedding]
backend = "none"

[knowledge_graph]
enabled = false
TOML

# ── Start server ────────────────────────────────────────────────────────────
echo "=== Starting agentrete ==="
"$BIN" -c "$CONFIG_DIR/config.toml" mcp --port "$PORT" &
PID=$!

for i in $(seq 1 20); do
    if http_get "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then
        echo "Ready after ${i}s"
        break
    fi
    sleep 1
done

# ── Health check ────────────────────────────────────────────────────────────
HEALTH=$(http_get "http://127.0.0.1:$PORT/")
echo "$HEALTH" | grep -q '"status":"ok"' || { echo "FAIL: health — $HEALTH"; exit 1; }
echo "PASS: health"

# ── Save memory ─────────────────────────────────────────────────────────────
SAVE=$(http_post "http://127.0.0.1:$PORT/" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"ci smoke test","type":"test"}}}')
echo "$SAVE" | grep -q "Saved:" || { echo "FAIL: save — $SAVE"; exit 1; }
echo "PASS: save"

# ── Stats check ─────────────────────────────────────────────────────────────
STATS=$(http_post "http://127.0.0.1:$PORT/" \
    '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_stats","arguments":{}}}')
echo "$STATS" | grep -q "Memories: 1" || { echo "FAIL: stats — $STATS"; exit 1; }
echo "PASS: stats"

echo "=== Smoke test PASSED ==="
