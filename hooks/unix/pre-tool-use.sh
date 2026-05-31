#!/bin/sh
HOOK_DIR="$(cd "$(dirname "$0")" && pwd)"; . "$HOOK_DIR/_json_extract.sh"
input=$(cat 2>/dev/null)
tool=$(json_val "$input" 'tool_name' '')

[ "$tool" != "Bash" ] && exit 0

cmd=$(json_val "$input" 'tool_input.command' '')

# ─── Blocklist: prevent sed/python3 from modifying source files ──────────────
# These patterns match sed -i / python3 -c ... >file / python3 -c with write operations
blocked=false

# sed -i (in-place edit)
if echo "$cmd" | grep -qE 'sed\s+.*-i'; then
    blocked=true
fi

# python3 -c modifying a file (redirect/sys.stdout write/open(w)/Path.write)
if echo "$cmd" | grep -qE 'python3\s+-c' && echo "$cmd" | grep -qE 'open\(.*["\x27]w["\x27]|\.write_text\(|sys\.stdout\b'; then
    blocked=true
fi

# python3 -c with apply_patch/template/subprocess towards source
if echo "$cmd" | grep -qE 'python3\s+-c.*(apply_patch|write|sed|replace)' && echo "$cmd" | grep -qE '\.rs\b|\.toml\b|\.json\b|\.yaml\b|\.yml\b|\.sh\b|\.py\b|\.md\b'; then
    blocked=true
fi

if $blocked; then
    cat >&2 << 'BLOCKED'
╔══════════════════════════════════════════════════════════════╗
║                    BLOCKED BY AGENTRETE                     ║
╠══════════════════════════════════════════════════════════════╣
║  Do NOT use sed or python3 to modify source files.         ║
║                                                            ║
║  Use apply_patch (Unified Diff) instead:                   ║
║    - Has context-line validation                           ║
║    - Fails with .rej files on conflict                     ║
║    - Won't silently corrupt code                           ║
║                                                            ║
║  Or rewrite the entire file if apply_patch is unavailable. ║
╚══════════════════════════════════════════════════════════════╝
BLOCKED
    echo "BLOCKED_COMMAND=$cmd" >&2
    exit 1
fi

# ─── Normal path ─────────────────────────────────────────────────────────────
echo "🔧 agentrete: $cmd" >&2
exit 0
