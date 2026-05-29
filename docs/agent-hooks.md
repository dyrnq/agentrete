# Agentrete Hooks — Complete Reference

> **Official references**:
> - [Codex CLI Hooks](https://developers.openai.com/codex/hooks)
> - [Claude Code Hooks](https://code.claude.com/docs/en/hooks)

## Overview

Hooks are platform-specific shell scripts (bash on Unix, PowerShell on Windows) that execute automatically at specific points in an AI coding agent's lifecycle. Agentrete packages hooks for **Codex CLI** (9 events) and **Claude Code** (2 events), with all scripts embedded into the binary at compile time.

When `agentrete setup` runs, it detects installed AI tools and writes the correct hook scripts + configuration to their respective directories.

## Supported Agents

| Agent | Hook Format | Events | Config File |
|-------|------------|--------|-------------|
| Codex CLI | JSON → `hooks.codex.json` | 9 | `$HOME/.codex/plugins/agentrete/hooks/` |
| Claude Code | JSON → `settings.json` | 2 | `$HOME/.claude/hooks/` + `settings.json` |
| Cursor | MCP tools only | 0 | — |
| Zed | MCP tools only | 0 | — |
| OpenCode | MCP tools only | 0 | — |
| Windsurf | MCP tools only | 0 | — |
| Goose | MCP tools only | 0 | — |
| Gemini CLI | MCP tools only | 0 | — |

---

## Codex CLI Hooks

### Directory Layout

```
Unix (~/.codex/plugins/agentrete/hooks/)
├── hooks.codex.json          Hook manifest (bash, ${HOME} paths)
├── scripts/
│   ├── session-start.sh      Load project context
│   ├── prompt-submit.sh      Search memories on user input
│   ├── pre-tool-use.sh       Pre-write handler (no-op)
│   ├── post-tool-use.sh      Auto-save write/exec operations
│   ├── pre-compact.sh        Snapshot before compaction
│   ├── post-compact.sh       Reload after compaction
│   ├── subagent-start.sh     Load rules for subagents
│   ├── subagent-stop.sh      Save subagent completion
│   └── stop.sh               No-op

Windows (%USERPROFILE%\.codex\plugins\agentrete\hooks\)
├── hooks.codex.json          Hook manifest (powershell, %USERPROFILE% paths)
├── scripts/
│   ├── session-start.ps1
│   ├── prompt-submit.ps1
│   ├── pre-tool-use.ps1
│   ├── post-tool-use.ps1
│   ├── pre-compact.ps1
│   ├── post-compact.ps1
│   ├── subagent-start.ps1
│   ├── subagent-stop.ps1
│   └── stop.ps1
```

### Hook Manifest (hooks.codex.json)

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/session-start.sh",
            "statusMessage": "agentrete: loading context"
          }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/prompt-submit.sh",
            "statusMessage": "agentrete: recalling memories"
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Edit|Write|Bash|exec_command|apply_patch",
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/pre-tool-use.sh"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit|Write|Bash|exec_command|apply_patch|mcp__agentrete__*",
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/post-tool-use.sh",
            "statusMessage": "agentrete: saving observation"
          }
        ]
      }
    ],
    "PreCompact": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/pre-compact.sh",
            "statusMessage": "agentrete: snapshotting context"
          }
        ]
      }
    ],
    "PostCompact": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/post-compact.sh",
            "statusMessage": "agentrete: reloading memories"
          }
        ]
      }
    ],
    "SubagentStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/subagent-start.sh",
            "statusMessage": "agentrete: loading for subagent"
          }
        ]
      }
    ],
    "SubagentStop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/subagent-stop.sh",
            "statusMessage": "agentrete: saving subagent observations"
          }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh ${HOME}/.codex/plugins/agentrete/hooks/scripts/stop.sh"
          }
        ]
      }
    ]
  }
}
```

### Event Reference

#### SessionStart

- **When**: Codex session begins or resumes
- **Matcher values**: `startup`, `resume`, `clear`, `compact`
- **Our behavior**: Detects project name via `git rev-parse --show-toplevel`, searches agentrete for project-related memories, outputs to stderr. Results cached per-project for 1 hour.

#### UserPromptSubmit

- **When**: User submits a prompt, before Codex processes it
- **Matcher**: None (always fires)
- **Our behavior**: Reads stdin JSON, extracts first ~120 chars of prompt text, calls `memory_search` with that query. Outputs top 3 results to stderr.

#### PreToolUse

- **When**: Before a tool call executes
- **Matcher**: Tool name (`Edit|Write|Bash|exec_command|apply_patch`)
- **Our behavior**: No-op placeholder. Reserved for future permission control / pre-validation.

#### PostToolUse

- **When**: After a tool call succeeds
- **Matcher**: Tool name (`Edit|Write|Bash|exec_command|apply_patch|mcp__agentrete__*`)
- **Our behavior**: Reads stdin JSON to extract tool name. If it's a write/exec tool, calls `memory_save` with `type=fact` and `tags=hook,tool-call`. Read-only tools (Read, Grep, Glob) are filtered out.

#### PreCompact

- **When**: Before context compaction
- **Matcher values**: `manual`, `auto`
- **Our behavior**: Calls `memory_save` to snapshot current context as a fact entry with `tags=hook,compact`.

#### PostCompact

- **When**: After compaction completes
- **Matcher values**: `manual`, `auto`
- **Our behavior**: Re-searches project memories to reload context after compaction cleared it.

#### SubagentStart

- **When**: A subagent is spawned
- **Matcher values**: `general-purpose`, `Explore`, `Plan`, or custom agent names
- **Our behavior**: Searches memories for project + "rules" to load coding standards into the subagent context.

#### SubagentStop

- **When**: A subagent finishes
- **Matcher values**: Same as SubagentStart
- **Our behavior**: Saves a completion marker `"Subagent completed"` as `type=fact` with `tags=hook,subagent`.

#### Stop

- **When**: Codex finishes responding for the turn
- **Matcher**: None (always fires)
- **Our behavior**: No-op.

---

## Claude Code Hooks

### Directory Layout

```
Unix ($HOME/.claude/hooks/)
├── agentrete-startup.sh      SessionStart: load project context
└── agentrete-post-tool.sh    PostToolUse: auto-save write operations

Windows (%USERPROFILE%\.claude\hooks\)
├── agentrete-startup.ps1
└── agentrete-post-tool.ps1
```

### Settings Integration

Claude Code hooks are registered in `$HOME/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "sh /home/user/.claude/hooks/agentrete-startup.sh"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "Edit|Write|Bash",
        "hooks": [
          {
            "type": "command",
            "command": "sh /home/user/.claude/hooks/agentrete-post-tool.sh"
          }
        ]
      }
    ]
  }
}
```

### Event Reference

#### SessionStart (Claude Code)

- **When**: Session begins
- **Behavior**: Same as Codex — detect project, search memories, cache 1h. Outputs to stderr with exit code 2 (Claude Code's convention for displaying hook output immediately).

#### PostToolUse (Claude Code)

- **When**: After a tool call succeeds
- **Matcher**: `Edit|Write|Bash`
- **Behavior**: Same as Codex — check tool name, auto-save write/exec operations.

---

## Hook Script Architecture

All hook scripts follow the same pattern:

```bash
#!/bin/sh
AGENTRETE_URL="${AGENTRETE_URL:-http://127.0.0.1:9092}"

# 1. Parse input from stdin (JSON from the agent)
# 2. Call agentrete MCP endpoint via curl
# 3. Output context to stderr (agents display stderr to model)
exit 0
```

### Key Design Decisions

1. **No external dependencies beyond sh+curl**: Scripts use only POSIX shell, curl, and optional python3 for JSON parsing. Windows uses native PowerShell with `Invoke-RestMethod`.

2. **Stateless hooks with cache files**: SessionStart hooks use `/tmp` cache files to avoid calling agentrete on every turn. Cache TTL is 1 hour.

3. **AGENTRETE_URL environment variable**: All scripts respect `AGENTRETE_URL` env var, defaulting to `http://127.0.0.1:9092`. Users with custom ports can override.

4. **Write-only hooks fire silently**: PostToolUse operations happen asynchronously and don't interrupt the agent. The save is fire-and-forget.

5. **Read-only tool filtering**: PostToolUse scripts check the tool name and skip read-only tools (Read, Grep, Glob) to avoid noise.

6. **Compile-time embedding**: All scripts are embedded via Rust's `include_str!()` macro. No external files needed at deploy time — `agentrete setup` writes them out.

---

## Comparison: Codex vs Claude Code Hooks

| Feature | Codex CLI | Claude Code |
|---------|-----------|-------------|
| Hook config format | JSON (`hooks.codex.json`) | JSON (`settings.json`) |
| Number of events | 9 supported | 2 supported |
| Matcher syntax | Regex (`Edit\|Write`) | Same |
| stdin input | JSON event context | JSON event context |
| stderr display | Exit code 0 | Exit code 2 for instant display |
| SessionStart | ✅ | ✅ |
| UserPromptSubmit | ✅ | ❌ |
| PreToolUse | ✅ (no-op) | ❌ |
| PostToolUse | ✅ | ✅ |
| PreCompact | ✅ | ❌ |
| PostCompact | ✅ | ❌ |
| SubagentStart | ✅ | ❌ |
| SubagentStop | ✅ | ❌ |
| Stop | ✅ (no-op) | ❌ |
| SessionEnd | ❌ | ❌ |
| Notification | ❌ | ❌ |
| PermissionRequest | ❌ | ❌ |
| Windows support | ✅ (PowerShell) | ✅ (PowerShell via `settings.json`) |
| Unix support | ✅ (bash) | ✅ (bash) |

## Troubleshooting

### Hooks not firing

1. Check hook scripts exist: `ls ~/.codex/plugins/agentrete/hooks/scripts/`
2. Check permissions: `chmod +x ~/.codex/plugins/agentrete/hooks/scripts/*.sh`
3. Check agentrete MCP is running: `curl http://127.0.0.1:9092/`
4. Check hook config is registered: `cat ~/.codex/plugins/agentrete/hooks/hooks.codex.json`
5. Run a hook script manually to test:
   ```bash
   echo '{"prompt":"test"}' | sh ~/.codex/plugins/agentrete/hooks/scripts/prompt-submit.sh
   ```

### Port already in use

If another process is on 9092, set `AGENTRETE_URL` before starting Codex:
```bash
export AGENTRETE_URL=http://127.0.0.1:9093
```

### Slow session start

The 1-hour cache in SessionStart hooks prevents repeated calls. If you want fresh context sooner, delete the cache:
```bash
rm /tmp/agentrete-startup-*.cache
```

### Reinstalling hooks

Run `agentrete setup` again. It will re-detect tools and re-write hook files.
