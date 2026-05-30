#!/bin/sh
# Public domain — shared JSON helpers for agentrete hooks.
# Usage: . _json_extract.sh

# ─── Tool detection ──────────────────────────────────────────────────────────

HAS_PYTHON3=false; command -v python3 >/dev/null 2>&1 && HAS_PYTHON3=true
HAS_JQ=false;      command -v jq      >/dev/null 2>&1 && HAS_JQ=true

# ─── json_val <body> <jq-style-path> [default] ───────────────────────────────
# Extracts a scalar value. Tries: python3 → jq → default.
json_val() {
    body="$1" path="$2" default="${3:-}"

    if $HAS_PYTHON3; then
        python3 -c "
import sys,json
v = json.load(sys.stdin)
try:
    for k in '${path}'.split('.'):
        v = v[int(k)] if k.isdigit() and isinstance(v,list) else v[k]
    print(v)
except: print('${default}')
" <<EOF
$body
EOF
        return
    fi

    if $HAS_JQ; then
        jq -r ".$path // \"$default\"" <<EOF 2>/dev/null
$body
EOF
        return
    fi

    echo "$default"
}

# ─── json_lines <body> <jq-array-path> ───────────────────────────────────────
# Extracts array items, one per line. Tries: python3 → jq → (no fallback).
json_lines() {
    body="$1" path="$2"

    if $HAS_PYTHON3; then
        python3 -c "
import sys,json
v = json.load(sys.stdin)
try:
    for k in '${path}'.split('.'):
        v = v[int(k)] if k.isdigit() and isinstance(v,list) else v[k]
    for x in v:
        print(x if isinstance(x,str) else json.dumps(x,ensure_ascii=False))
except: pass
" <<EOF
$body
EOF
        return
    fi

    if $HAS_JQ; then
        jq -r ".$path[]?" <<EOF 2>/dev/null
$body
EOF
        return
    fi
}

# ─── mcp_post <url> <json_body> ──────────────────────────────────────────────
# POST to agentrete. Tries curl → wget.
mcp_post() {
    url="$1" json_body="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -s -X POST "$url" -H "Content-Type: application/json" -d "$json_body" 2>/dev/null
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- --header="Content-Type: application/json" --post-data="$json_body" "$url" 2>/dev/null
    fi
}

# ─── detect_project ──────────────────────────────────────────────────────────
# Detects project name: git repo basename → current dir basename → "unknown"
detect_project() {
    if command -v git >/dev/null 2>&1; then
        local repo
        repo=$(git rev-parse --show-toplevel 2>/dev/null) && {
            basename "$repo"
            return
        }
    fi
    basename "${PWD:-$(pwd)}" 2>/dev/null || echo "unknown"
}
