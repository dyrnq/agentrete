# DuckDB Concurrency & Locking

## The Problem

DuckDB allows only **one writer process** at a time. Multiple processes attempting to open the same `.db` file in read-write mode will fail with:

```
IO Error: Could not set lock on file "memory.db": Conflicting lock is held in PID 12345
```

This happens when:
- The systemd MCP service is running (`agentrete mcp --port 9092`)
- You run a CLI command like `agentrete save "test"` from another terminal
- Or multiple Codex instances try to write simultaneously

## Why Agentrete Is Affected

Agentrete uses DuckDB as an embedded OLAP database. Unlike client-server databases (PostgreSQL, MySQL), DuckDB is **in-process** — the database engine runs inside the application binary. This means:

- One writer process = one exclusive file lock
- Read-only access is allowed concurrently
- Write access is serial

## Our Architecture

```
┌─────────────────────┐
│  systemd service    │  ← Single writer (always running)
│  agentrete mcp      │
│  --port 9092        │
└─────────┬───────────┘
          │ HTTP :9092
          ▼
┌─────────────────────┐
│  Codex instance A   │  ← MCP client (HTTP), no direct DB access
│  Codex instance B   │
│  Codex instance C   │
└─────────────────────┘
          │
┌─────────────────────┐
│  CLI commands       │  ← Direct DB access → WILL CONFLICT
│  agentrete save     │     with the systemd service!
│  agentrete search   │
└─────────────────────┘
```

## Solutions

### Current: Stop Service Before CLI

```bash
systemctl --user stop agentrete
agentrete save "some memory"
systemctl --user start agentrete
```

**Pros**: Simple, no code changes
**Cons**: Manual, MCP unavailable during CLI use

### Recommended: CLI via HTTP

Make CLI commands talk to the MCP HTTP service instead of opening DuckDB directly:

```rust
// Instead of:
let store = Store::open().await?;
store.save(memory).await?;

// Do:
reqwest::post("http://127.0.0.1:9092/")
    .json(&jsonrpc_request)
    .send()?;
```

**Pros**: Zero lock conflicts, CLI + MCP coexist
**Cons**: CLI depends on MCP service being up

### Alternative: Read-Only Fallback

DuckDB supports read-only mode for concurrent access. We could modify `Store::open()` to:

1. Try ReadWrite first
2. On lock conflict, fall back to ReadOnly
3. Write operations (save, forget, wipe) return error in read-only mode

**Pros**: CLI can read while MCP writes
**Cons**: CLI can't write; complexity in error handling

### Not Viable: WAL Mode

Unlike SQLite, DuckDB **does not support** Write-Ahead Logging (WAL) for concurrent writers. This is a deliberate design choice — DuckDB optimizes for single-writer analytics workloads.

## Best Practices

1. **Use the MCP service** as the single source of truth for all write operations
2. **Route CLI commands through HTTP** to avoid lock contention
3. **Never run multiple MCP instances** against the same database file
4. **If using stdio MCP mode**, be aware that each Codex instance spawns its own agentrete process — they will conflict on the same DB

## References

- [DuckDB Concurrency Docs](https://duckdb.org/docs/stable/connect/concurrency)
- [DuckDB vs SQLite WAL](https://duckdb.org/2024/12/06/duckdb-vs-sqlite.html)
- [Agentrete Architecture](./architecture.md)
