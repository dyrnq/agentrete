# Code Modification Rules (BREAK GLASS — CRITICAL)

**ABSOLUTELY FORBIDDEN** — these patterns will corrupt code:

- `sed -i ...` — in-place regex editing, fails silently on edge cases
- `python3 -c "..."` — modifying source files via Python, no validation
- Shell heredocs overwriting `.rs` / `.toml` / `.json` / `.yaml` files
- Any command that writes to source files without context validation

**REQUIRED instead**:

- `apply_patch` tool with Unified Diff format — has context-line validation
- If apply_patch is unavailable, rewrite the entire file with `cat > file << 'EOF'`
- Always confirm original content matches context lines before patching

**Why**: sed/python3 code modification has near 100% failure rate on non-trivial changes.
Unified Diff fails with `.rej` files on conflict — it never silently corrupts.

**After every code change**, run in order:
1. `cargo fmt`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo build`
If any step fails, revert immediately.

---

## Documentation

- Never use private paths (`/home/bill`, `192.168.x.x`) in docs — use `~` and `localhost`
- Use `ast-grep` for code navigation instead of reading whole files

---

## Agentrete Memory

MCP service at `http://127.0.0.1:9092/`. Search memories at the start of each conversation:

```
memory_search(query="<task keywords>", limit=5)
```

Types: `rule` (coding standards), `decision` (architecture), `pattern` (recurring), `bug` (fixed), `fact` (context).

---

## Project Context (agentrete)

- Rust project, `cargo build` for debug, `cargo test` for tests
- SQLite + sqlx + FTS5 for storage, axum for HTTP, candle for local embedding
- Config via `config-rs` (TOML/YAML/JSON + env), nested `[embedding.remote]` structure
- Embed worker runs only in HTTP mode, stdio skips embedding
- Default local model: `BAAI/bge-small-zh-v1.5` (512d, 93MB)
