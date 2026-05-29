# Agentrete Test Plan

Run after any code change. All tests must pass before commit.

## Prerequisites

- Rust toolchain (stable)
- Python 3.11+
- `curl`, `jq`
- Model2Vec distilled model at `/tmp/m2v-bge-small-zh`
- No process on port 9092 before starting

## Test Config

```toml
# /tmp/test-m2v.toml
port = 9092
db_dir = "/tmp/test-db"

[embedding]
backend = "model2vec"

[embedding.local]
model = "BAAI/bge-small-zh-v1.5"
dims = 256
model2vec_path = "/tmp/m2v-bge-small-zh"
```

---

## Phase 1: Build & Lint

```bash
cd /data/work/agentrete
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build
```

**Expected**: All three pass with zero errors.

---

## Phase 2: Startup & Health

```bash
rm -rf /tmp/test-db
RUST_LOG=info cargo run -- -c /tmp/test-m2v.toml mcp --port 9092 &
sleep 3
curl -s http://127.0.0.1:9092/
```

**Expected**: `{"service":"agentrete","status":"ok","version":"..."}`  
**Log check**: `Model2Vec loaded: /tmp/m2v-bge-small-zh (256d)`  
**Log check**: `sqlite-vec extension extracted to /tmp/agentrete/vec0-linux-x86_64.so`

---

## Phase 3: Save & Stats

```bash
curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"不要用sed修改代码 必须用apply_patch","type":"rule","tags":"code-rule"}}}' \
  -H "Content-Type: application/json"

curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"Never use sed to modify source code, always use apply_patch","type":"rule","tags":"code-rule"}}}' \
  -H "Content-Type: application/json"

curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"今天天气真好适合出去玩","type":"test"}}}' \
  -H "Content-Type: application/json"

curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"memory_stats","arguments":{}}}' \
  -H "Content-Type: application/json"
```

**Expected**: 3 saves return `"Saved: mem_..."`. Stats show 3 memories.

---

## Phase 4: Semantic Search

### 4.1 Chinese → Chinese

```bash
curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"不要用sed修改源代码 使用apply_patch","limit":3}}}' \
  -H "Content-Type: application/json"
```

**Expected**: Top result is Chinese rule `不要用sed修改代码` with score ≥ 0.85.

### 4.2 English → English

```bash
curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"Never use sed to modify code","limit":3}}}' \
  -H "Content-Type: application/json"
```

**Expected**: Top result is English rule `Never use sed...` with score ≥ 0.85.

### 4.3 Cross-lingual: English → Chinese

**Expected**: Both Chinese and English rules appear in top 5.  
English rule score ≥ 0.90, Chinese rule score ≥ 0.40.

### 4.4 Cross-lingual: Chinese → English

**Expected**: Both rules appear. Chinese rule score ≥ 0.90, English rule score ≥ 0.40.

### 4.5 Unrelated Query

```bash
curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"今天天气非常好","limit":3}}}' \
  -H "Content-Type: application/json"
```

**Expected**: Top result matches irrelevant Chinese text with score ≥ 0.70.  
Code rules do not appear in top 3 (robustness: irrelevant query → not confused with rules).

---

## Phase 5: Concurrent Write Stress

```bash
python3 << 'PYEOF'
import concurrent.futures, urllib.request, json, time

URL = "http://127.0.0.1:9092/"
TOTAL = 1000
CONCURRENT = 10

def do_save(i):
    payload = json.dumps({
        "jsonrpc": "2.0", "id": i,
        "method": "tools/call",
        "params": {"name": "memory_save", "arguments": {"content": f"stress-{i:04d}", "type": "test"}}
    }).encode()
    try:
        req = urllib.request.Request(URL, data=payload, headers={"Content-Type": "application/json"})
        urllib.request.urlopen(req, timeout=30)
        return True
    except:
        return False

start = time.time()
with concurrent.futures.ThreadPoolExecutor(max_workers=CONCURRENT) as ex:
    results = list(ex.map(do_save, range(TOTAL)))
elapsed = time.time() - start
success = sum(results)
assert success == TOTAL, f"FAIL: {success}/{TOTAL}"
print(f"PASS: {TOTAL} req in {elapsed:.1f}s ({TOTAL/elapsed:.0f} req/s)")
PYEOF
```

**Expected**: 1000/1000 success, ≥ 50 req/s.

### 5.2 High Concurrency

Same as above with `CONCURRENT = 20`. Still expect 100% success.

---

## Phase 6: Embed Worker Progress

```bash
# Wait for embed worker to process pending rows
sleep 30

curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"memory_stats","arguments":{}}}' \
  -H "Content-Type: application/json"
```

**Expected**: `embeddings > 0`. All memories eventually have embeddings.

**Log check**: `embed-worker: flushed N vectors` appears.

---

## Phase 7: MCP Tools Smoke Test

### memory_list

```bash
curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"memory_list","arguments":{"limit":3}}}' \
  -H "Content-Type: application/json"
```

**Expected**: Returns 3 recent memories.

### memory_forget

```bash
# Get a memory ID from list, then:
curl -s -X POST http://127.0.0.1:9092/ \
  -d '{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"memory_forget","arguments":{"id":"MEM_ID"}}}' \
  -H "Content-Type: application/json"
```

**Expected**: `"Deleted: MEM_ID"`.

---

## Phase 8: Shutdown

```bash
pkill -f "target/debug/agentrete"
sleep 2
rm -rf /tmp/test-db
```

**Expected**: No orphaned agentrete processes. Port 9092 free.

---

## Full Automated Run

```bash
# Run all phases except Phase 8 (cleanup)
# Copy to /tmp/test-agentrete.sh and execute:
bash /tmp/test-agentrete.sh
```

---

## Regression Checklist

| Test | Assertion |
|------|-----------|
| `cargo fmt` | Zero changes |
| `cargo clippy -- -D warnings` | Zero errors |
| `cargo build` | Compiles |
| Health endpoint | Returns 200 with version |
| Model2Vec loaded | Log shows model path + dims |
| sqlite-vec loaded | Log shows extension extracted |
| `memory_save` | Returns `Saved: mem_...` |
| `memory_stats` | Shows correct memory count |
| `memory_search` zh→zh | Top score ≥ 0.85 |
| `memory_search` en→en | Top score ≥ 0.85 |
| `memory_search` en→zh | Cross-lingual hit (score ≥ 0.40) |
| `memory_search` zh→en | Cross-lingual hit (score ≥ 0.40) |
| `memory_search` irrelevant | No rule false positives |
| Concurrent write 1000×10 | 100% success |
| Concurrent write 1000×20 | 100% success |
| Embed worker progress | embeddings count increases |
| `memory_list` | Returns items |
| `memory_forget` | Deletes item |
| Shutdown | No orphaned processes |

---

## Phase 9: stdio Transport

### 9.1 Startup

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}' | cargo run -- -c /tmp/test-m2v.toml mcp 2>/dev/null
```

**Expected**: Returns `initialize` response with `protocolVersion`, `serverInfo`, `capabilities`.

### 9.2 Tool List

```bash
echo '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | cargo run -- -c /tmp/test-m2v.toml mcp 2>/dev/null
```

**Expected**: Returns array with `memory_save`, `memory_search`, `memory_list`, `memory_forget`, `memory_stats`, `memory_compact`.

### 9.3 Save via stdio

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_save","arguments":{"content":"stdio-test","type":"test"}}}\n' | cargo run -- -c /tmp/test-m2v.toml mcp 2>/dev/null | grep "Saved"
```

**Expected**: Output contains `"Saved: mem_..."`.

### 9.4 Search via stdio

```bash
printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"memory_search","arguments":{"query":"stdio test","limit":3}}}\n' | cargo run -- -c /tmp/test-m2v.toml mcp 2>/dev/null | grep "stdio-test"
```

**Expected**: Output contains `stdio-test` in search results.

### 9.5 Embed Worker Disabled

**Expected**: No `embed-worker: started` in stderr when using stdio transport (embed worker only spawns for HTTP).

### 9.6 Empty Input

```bash
echo "" | cargo run -- -c /tmp/test-m2v.toml mcp 2>/dev/null
```

**Expected**: No crash. Gracefully ignores empty lines.

## Regression Checklist (stdio)

| Test | Assertion |
|------|-----------|
| `initialize` | Returns protocol version + capabilities |
| `tools/list` | Returns 6 tools |
| `memory_save` via stdio | Returns saved ID |
| `memory_search` via stdio | Returns matching results |
| Embed worker | Not spawned in stdio mode |
| Empty input | No crash, graceful ignore |
