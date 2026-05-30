#!/usr/bin/env python3
"""Insert 10,000 test memories into agentrete SQLite DB for stress testing."""
import sqlite3, uuid, time, sys

DB = sys.argv[1] if len(sys.argv) > 1 else "/tmp/test-db/memory.db"
BATCH = 500
TOTAL = 10000

conn = sqlite3.connect(DB)
cur = conn.cursor()
start = time.time()

for i in range(0, TOTAL, BATCH):
    rows = []
    now = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime())
    for j in range(BATCH):
        idx = i + j
        rid = "mem_" + uuid.uuid4().hex[:12]
        content = "stress-" + str(idx).zfill(5) + " " + uuid.uuid4().hex[:8]
        rows.append((rid, "test", content, "[]", 3, now, now))
    cur.executemany(
        "INSERT OR IGNORE INTO memories (id,type,content,tags,importance,created_at,updated_at) VALUES (?,?,?,?,?,?,?)",
        rows,
    )
    conn.commit()
    if (i + BATCH) % 2000 == 0:
        e = time.time() - start
        print(f"  {i+BATCH}/{TOTAL} in {e:.1f}s")

conn.close()
print(f"Done: {TOTAL} rows inserted into {DB}")
