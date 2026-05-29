# Agentrete Memory Decision Guide

## Memory Sources

Agentrete acquires memories through three paths:

```
User explicit save ──→  memory_save("remember: xxx")
                         │
Codex autonomous call ──→  judges value → calls memory_save
                         │
Hook auto-record ──────→  PostToolUse → filter → memory_save
```

## Path 1: Hook Auto-Record

**Trigger**: Codex executes a write-type tool call (Write, Edit, exec_command, apply_patch).

**Filter rules** (excluded operations):

| Operation | Reason |
|-----------|--------|
| Read | Read-only, no information value |
| Glob | Read-only |
| Grep | Read-only |
| Bash (pure query) | No side effects |
| Task | Internal scheduling |
| AskUserQuestion | Temporary interaction |

**Recorded content**: `Tool call: {tool_name}` + `type=fact` + `tags=hook,tool-call`

**Examples**:

- ✅ `Edit: src/mcp.rs` → auto-recorded
- ✅ `exec_command: cargo build` → auto-recorded
- ❌ `Read: config.rs` → not recorded

**All 9 Codex hook events**:

| Event | Action |
|-------|--------|
| SessionStart | Load project context from memories (cached 1h) |
| UserPromptSubmit | Search memories with prompt keywords |
| PreToolUse | No-op |
| PostToolUse | Auto-save write operations |
| PreCompact | Snapshot current context |
| PostCompact | Reload project memories |
| SubagentStart | Load project rules for subagent |
| SubagentStop | Save subagent completion marker |
| Stop | No-op |

## Path 2: Codex Autonomous Save

Codex judges whether information is worth long-term storage based on conversation context and proactively calls `memory_save`.

**Types suitable for autonomous save**:

| type | Content | Example |
|------|---------|---------|
| `rule` | Development standards, coding preferences | "Never use sed to modify code" |
| `decision` | Architecture decisions, technology choices | "Choose m3e-base as default model" |
| `pattern` | Recurring problem patterns | "libduckdb-sys compile slow, need opt-level=3" |
| `fact` | Key environment information | "Project needs HF_ENDPOINT mirror" |
| `bug` | Fixed bug records | "DuckDB FLOAT[] type mismatch issue" |

**Do NOT auto-save**:

- Temporary debug information
- Conversation context (managed by Codex itself)
- Content already documented in files
- One-time operations (e.g., "check the weather")

## Path 3: User Explicit Save

Say "remember: xxx" or use the `memory_save` tool directly.

## Anti-Patterns: What NOT to Record

| Anti-pattern | Reason | Improvement |
|--------------|--------|-------------|
| Record every Read | Noise, interferes with search | Hook already filters |
| Save code snippets | Should be in files, memory stores decisions | Use `type=decision` for reasons |
| Save full logs | Too large, no search value | Summarize key points then save |
| Repeatedly save the same rule | Redundant | Search before saving |

## Memory Lifecycle

1. **Write**: Three paths enter `memories` table + auto-compute 768-dim vector
2. **Search**: BM25 full-text + vector semantic hybrid search
3. **Expiry**: Currently no auto-expiry, manual `forget` or `wipe`

## Best Practices

- Preferences/standards → `type=rule`
- Technical decisions → `type=decision`
- Auto-records → `type=fact`
- Search relevant memories at session start
- Periodic review: `agentrete list` to check recent entries, clean up noise
