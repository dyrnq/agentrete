# Agentrete Test Plan

Run after any code change. All tests must pass before commit.

## Prerequisites

- Rust toolchain (stable)
- `curl`, `jq`
- `ast-grep` (sg) — `cargo install ast-grep` (required for `kg_scan`)
- `git` (required for `kg_scan` git history extraction)
- No process on port 9092 before starting

## Config

```toml
# /tmp/test-m2v.toml
port = 9092
db_dir = "/tmp/test-db"

[embedding]
backend = "model2vec"

[embedding.model2vec]
model = "BAAI/bge-small-zh-v1.5"
dims = 256
model2vec_path = "~/.cache/model2vec/bge-small-256d"

[knowledge_graph]
enabled = true
```

---

## Phase 1: Build & Lint

```bash
cd /path/to/agentrete
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

**Expected**: All three pass with zero errors.

---

## Phase 2: Unit Tests

```bash
cargo test
```

**Expected**: 33 test cases pass:

| Test | What it covers |
|------|----------------|
| `test_reembed_flow` | Embed model change detection, pending SQL logic |
| `test_scan_git_history` | Git log parsing → kg_triples (commit/message/author/file) |
| `test_basic_neighbors` | KG neighbor queries on in-memory graph |
| `test_path_same_node` | Shortest path when source=target |
| `test_path_no_connection` | Shortest path between disconnected nodes |
| `test_query_path` | kg_query path traversal |
| `test_disabled_graph` | KG gracefully handles disabled state |
| `test_no_relations` | Empty graph returns empty results |
| `test_extract_name` | AST symbol name extraction |
| `test_extract_import_target` | Import/use/require parsing |
| `test_kind_to_symbol_kind` | AST kind → symbol type mapping |
| `test_confidence_and_source` | Triple metadata fields |
| `test_register_and_complete` | TaskManager lifecycle: register → complete |
| `test_cancel_task` | TaskManager cancel flag flip |
| `test_fail_task` | TaskManager fail with error message |
| `test_cancel_nonexistent` | Cancel on unknown task returns false |
| `test_all_statuses` | TaskManager lists all registered tasks |
| `test_protocol_includes_key_sections` | MCP instructions doc has all sections |
| `test_detect_openai` | Remote embed vendor detection |
| `test_remote_vendor_explicit` | Config override for remote vendor |

Plus ~12 KG node/edge/symbol tests and remote Ollama tests.

---

## Phase 3: Startup & Health

```bash
# Build binary first
cargo build

# Create config (adjust model2vec_path for your environment)
cat > /tmp/test-m2v.toml << 'EOF'
port = 9092
db_dir = "/tmp/test-db"

[embedding]
backend = "model2vec"

[embedding.model2vec]
model = "BAAI/bge-small-zh-v1.5"
dims = 256
model2vec_path = "~/.cache/model2vec/bge-small-256d"

[knowledge_graph]
enabled = true
EOF

# Install and start as systemd daemon (recommended — stable, auto-restart)
agentrete daemon install --port 9092 --binary "$(pwd)/target/debug/agentrete"

# Override service to use the test config
mkdir -p ~/.config/systemd/user
cat > ~/.config/systemd/user/agentrete.service << 'SVC_EOF'
[Unit]
Description=Agentrete Memory Server (MCP)
After=network.target

[Service]
ExecStart=PATH_TO_BINARY -c /tmp/test-m2v.toml mcp --port 9092
Restart=on-failure
RestartSec=2
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
SVC_EOF

# Replace PATH_TO_BINARY with actual binary path
sed -i "s|PATH_TO_BINARY|$(pwd)/target/debug/agentrete|" ~/.config/systemd/user/agentrete.service

systemctl --user daemon-reload
systemctl --user restart agentrete.service
sleep 3
curl -s http://127.0.0.1:9092/
```

**Expected**: `{"service":"agentrete","status":"ok","version":"..."}`  
**Log check**: `sqlite-vec extension extracted to /tmp/agentrete/vec0-linux-x86_64.so`  
**Log check**: `Model2Vec loaded: ~/.cache/model2vec/bge-small-256d (256d)`  
**Log check**: `kg: built graph (N nodes, M edges)`  
**Log check** (if sg not installed): warning about missing ast-grep

---

## Phase 4: Initialize & Tools

```bash
# Check supported protocol versions
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"initialize","params":{"protocolVersion":"2025-11-25"},"id":1}' | jq .
```

**Expected**: `capabilities` contains `"tasks": {}` and `"tools": {"listChanged": false}`.

```bash
# List all tools
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/list","id":2}' | jq '.result.tools[].name'
```

**Expected**: 9 tools — `memory_search`, `memory_save`, `memory_list`, `memory_forget`, `memory_stats`, `memory_compact`, `kg_query`, `kg_scan`, `kg_scan_status`.

---

## Phase 5: Memory Operations

```bash
# Save
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"test memory","type":"test"}},"id":3}'
# Expected: "Saved: mem_..."

# Stats
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":4}'
# Expected: Memories: 1+

# List
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"memory_list","arguments":{"limit":10}},"id":5}'
# Expected: shows saved memory

# Forget
# Copy ID from list, then:
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"memory_forget","arguments":{"id":"mem_xxx"}},"id":6}'
# Expected: "Deleted: mem_xxx"
```

---

## Phase 6: Knowledge Graph

### 6.0 Prerequisites

ast-grep (sg) must be installed:

```bash
which sg || cargo install ast-grep
```

### 6.1 Scan Codebase

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tasks/send","params":{"name":"kg_scan","arguments":{"path":"/path/to/agentrete","watch":false}},"id":10}' | jq .
```
Expected: Returns task_0001 with status running.

### 6.2 Wait for Completion

```bash
sleep 10
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tasks/status","params":{"id":"task_0001"},"id":11}' | jq '.result.content[0].text'
```
Expected: completed with ok=true, symbols, relations.

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_scan_status","arguments":{}},"id":12}' | jq -r '.result.content[0].text'
```
Expected: "No scan running."

### 6.3 Query Neighbors

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":"agentrete"}},"id":13}' | jq -r '.result.content[0].text'
```
Expected: Relations like `agentrete --[in:contains]--> file:README`.

Empty entity (error case):

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":""}},"id":14}' | jq -r '.error.message'
```
Expected: Error requiring entity.

### 6.4 Query Path

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"path","entity":"agentrete","target":"MCP"}},"id":15}' | jq -r '.result.content[0].text'
```
Expected: Path or "No path found".

### 6.5 Scan with Watch

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tasks/send","params":{"name":"kg_scan","arguments":{"path":"/path/to/agentrete","watch":true}},"id":16}' | jq '.result.content[0].text'
```
Expected: Scan starts, file watcher activated.

```bash
sleep 10
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tasks/status","params":{"id":"task_0002"},"id":17}' | jq '.result.content[0].text'
```

### 6.6 Cancel Task

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tasks/cancel","params":{"id":"task_0001"},"id":18}' | jq '.result.content[0].text'
```
Expected: cancelled or not found.

### 6.7 KG Disabled Mode

Start server with `knowledge_graph.enabled = false`, then:

```bash
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":"test"}},"id":19}' | jq -r '.error.message'
```
Expected: "Knowledge graph is disabled."

---



Test that switching embedding model dimensions triggers full re-embed.

## Phase 7: Re-Embed Stress Test

### 7.1 Insert 10,000 Memories

With server still running from Phase 6, insert 10k rows:

```bash
python3 << 'PYEOF'
import sqlite3, uuid, time
DB = "/tmp/test-db/memory.db"
conn = sqlite3.connect(DB)
cur = conn.cursor()
batch_size = 500
total = 10000
start = time.time()
for i in range(0, total, batch_size):
    rows = []
    now = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime())
    for j in range(batch_size):
        idx2 = i + j
        rid = "mem_" + uuid.uuid4().hex[:12]
        content = "stress-" + str(idx2).zfill(5) + " " + uuid.uuid4().hex[:8]
        rows.append((rid, "test", content, "[]", 3, now, now))
    cur.executemany("INSERT OR IGNORE INTO memories (id,type,content,tags,importance,created_at,updated_at) VALUES (?,?,?,?,?,?,?)", rows)
    conn.commit()
    if (i + batch_size) % 2000 == 0:
        e = time.time() - start
        print(f"  {i+batch_size}/{total} in {e:.1f}s")
conn.close()
print(f"Done: {total} rows")
PYEOF
```

Verify:
```bash
curl -s http://127.0.0.1:9092/ -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":10}' | jq -r '.result.content[0].text' | grep Memories
```
Expected: Memories: 10000+

### 7.2 Embed Worker

```bash
sleep 15
curl -s http://127.0.0.1:9092/ -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":11}' | jq -r '.result.content[0].text'
```
Expected: Embedding count > 0.

### 7.3 Shutdown

```bash
pkill -f "agentrete.*mcp"
sleep 2
```

### 7.4 Switch Model (256d -> 512d)

```toml
# /tmp/test-m2v-v2.toml
port = 9092
db_dir = "/tmp/test-db"

[embedding]
backend = "model2vec"

[embedding.model2vec]
model = "BAAI/bge-small-zh-v1.5"
dims = 512
model2vec_path = "~/.cache/model2vec/bge-small-512d"

[knowledge_graph]
enabled = true
```

### 7.5 Restart with New Dims

```bash
rm -f /tmp/test-db/memory.db-wal /tmp/test-db/memory.db-shm
cargo run --bin agentrete -- -c /tmp/test-m2v-v2.toml mcp -p 9092 &
sleep 5
```

Log check: `init_vec: stored dims != 512, dropping vec0 + clearing embeddings`

```bash
curl -s http://127.0.0.1:9092/ -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":12}' | jq -r '.result.content[0].text'
```
Expected: 10000+ (0 embeddings)

### 7.6 Wait for Re-Embed

```bash
sleep 30
curl -s http://127.0.0.1:9092/ -d '{"method":"tools/call","params":{"name":"memory_stats","arguments":{}},"id":13}' | jq -r '.result.content[0].text'
```
Expected: 10000 embeddings, model shows 512d.

### 7.7 Search Still Works

```bash
curl -s http://127.0.0.1:9092/ -d '{"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"stress test memory","limit":5}},"id":14}' | jq -c '.result.content[0].text[:120]'
```
Expected: Results with scores > 0.

---

## Phase 8: KG Edge Cases

```bash
# KG disabled → graceful error
# Start server without kg, then:
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":"test"}},"id":13}' | jq .
```
**Expected**: Error message about KG being disabled.

```bash
# Empty entity
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tools/call","params":{"name":"kg_query","arguments":{"mode":"neighbors","entity":""}},"id":14}' | jq .
```
**Expected**: Error message requiring entity.

---

## Phase 9: Panic Protection

```bash
# Trigger a scan (runs in background task)
# If scan_codebase panics, the server should stay alive
curl -s http://127.0.0.1:9092/ \
  -d '{"method":"tasks/send","params":{"name":"kg_scan","arguments":{"path":"/nonexistent","watch":false}},"id":15}' | jq .

# Server should still respond
sleep 2
curl -s http://127.0.0.1:9092/ | jq .
```
**Expected**: Server continues responding after any task panic.

---

## Phase 10: Shutdown

```bash
pkill -f "agentrete.*mcp"
sleep 1
```

**Expected**: No orphaned processes.

---

## Regression Checklist

| Test | Assertion |
|------|-----------|
| `cargo fmt --check` | Zero changes |
| `cargo clippy -- -D warnings` | Zero errors |
| `cargo build` | Compiles |
| `cargo test` | 33 passed |
| Health endpoint | Returns 200 with version |
| `initialize` (2025-11-25) | `capabilities.tasks` present |
| `tools/list` | 9 tools |
| `memory_save` | Returns `Saved: mem_...` |
| `memory_stats` | Shows count |
| `memory_list` | Returns items |
| `memory_forget` | Deletes item |
| `tasks/send kg_scan` | Returns task with `running` |
| `tasks/status` | Shows `completed` with result |
| `kg_query neighbors` | Returns relations or empty |
| `kg_query path` | Returns path or not-found |
| `kg_scan watch=true` | Scan + watch starts |
| `tasks/cancel` | Returns cancelled or not-found |
| KG disabled mode | Graceful error |
| Panic protection | Server survives task crash |
| No orphan processes | Port 9092 free after kill |
