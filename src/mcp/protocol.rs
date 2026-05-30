//! Memory protocol — behavioral instructions that teach AI agents
//! how to use agentrete effectively. This is returned in the `instructions`
//! field of the MCP `initialize` response.

pub const MEMORY_PROTOCOL: &str = r#"AGENTRETE MEMORY PROTOCOL (for AI agents)
===============================================

Agentrete is a local-first persistent memory engine. All memories survive
restarts. Use it to remember decisions, patterns, rules, and bugs across
sessions.

## 0. WHEN TO SEARCH MEMORY

ALWAYS search memory at the start of a task before you begin work.
Use `memory_search` with a short query describing the task.

Exceptions (skip the search):
- Greetings, casual chat
- The user explicitly says "don't search memory"

Why this matters: agentrete stores coding rules, architecture decisions,
known bugs, and user preferences from past sessions. Skipping the search
is equivalent to starting from scratch every time.

## 1. WHEN TO SAVE MEMORY

Save to memory when:
- User says "remember", "save to memory", "log this", "this is important"
- An architecture decision is made (tech stack, API design, data model choice)
- A repetitive pattern is identified (with solution)
- A bug is found and fixed (so it stays dead)
- A coding rule or preference is stated

Do NOT save:
- Transient test results that will be stale tomorrow
- UI-level trivia that has no reuse value
- Code snippets — the file system is the source of truth

## 2. MEMORY TYPES

| type     | When to use                                    | Example                                  |
|----------|------------------------------------------------|------------------------------------------|
| `rule`   | Coding standards, preferences, prohibitions    | "Never use sed to modify source code"    |
| `decision`| Architecture and technology choices            | "Use sqlx instead of rusqlite"           |
| `pattern`| Recurring problems and their solutions         | "DuckDB lock — use WAL mode"            |
| `bug`    | Bugs that were fixed (so they don't recur)     | "vec0 init fails without foreign_keys"   |
| `fact`   | Environment info, config details, gotchas      | "Ollama at localhost:11434"           |

Use `tags` for categorization: comma-separated keywords like
`code-rule,workflow,agentrete`.

## 3. MEMORY FORMAT

Store meaningful, self-contained facts. Bad: "fixed it". Good:
"DuckDB concurrent access: use WAL journal mode and busy_timeout=5000
to avoid lock conflicts between MCP server and embed worker."

## 4. SEARCH RESULTS

`memory_search` returns results with similarity scores. Higher = better
semantic match. Use the top 3-5 results as context for the current task.

## 5. MEMORY MAINTENANCE

- Use `memory_compact` when the user asks to deduplicate or clean up.
  Mode `"exact"` removes exact content+type duplicates.
  Mode `"semantic"` merges near-duplicates by meaning.
- Call `memory_stats` to understand what's stored (type breakdown,
  model info, DB size).

## 6. NEVER DELETE WITHOUT PERMISSION

Use `memory_forget` ONLY when the user explicitly asks you to delete
a specific memory. Never delete memories on your own initiative,
even if you think they are outdated or low quality.

## 7. HOOK-BASED AUTO-CAPTURE

Agentrete hooks capture tool-call outcomes automatically.
You do NOT need to manually call `memory_save` after every tool use.
Only save explicitly when the user asks or when a decision is made.

## 8. KNOWLEDGE GRAPH (optional)

If `knowledge_graph` is enabled in config, use `kg_scan` to scan a
codebase and build a dependency/symbol graph. This requires `ast-grep`:

    cargo install ast-grep

Run `kg_scan path="." watch=true` to scan and watch for changes.
Use `kg_query` to traverse the graph (neighbors, path).
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_includes_key_sections() {
        assert!(MEMORY_PROTOCOL.contains("0. WHEN TO SEARCH"));
        assert!(MEMORY_PROTOCOL.contains("1. WHEN TO SAVE"));
        assert!(MEMORY_PROTOCOL.contains("2. MEMORY TYPES"));
        assert!(MEMORY_PROTOCOL.contains("6. NEVER DELETE"));
    }
}
