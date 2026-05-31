# agentrete PreToolUse hook — blocks sed/python3 code modification on Windows

$input = $input | Out-String
$json = $input | ConvertFrom-Json
$tool = $json.tool_name

if ($tool -ne "Bash") { exit 0 }

$cmd = $json.tool_input.command

$blocked = $false

if ($cmd -match 'sed\s+.*-i') { $blocked = $true }

if (($cmd -match 'python3?\s+-c') -and ($cmd -match "open\(.*['\x60]w['\x60]" -or $cmd -match '\.write_text\(' -or $cmd -match 'sys\.stdout\b')) {
    $blocked = $true
}

if (($cmd -match 'python3?\s+-c.*(apply_patch|write|sed|replace)') -and ($cmd -match '\.rs|\.toml|\.json|\.yaml|\.yml|\.sh|\.py|\.md')) {
    $blocked = $true
}

if ($blocked) {
    Write-Error @"
BLOCKING: Do NOT use sed/python3 to modify source files.
Use apply_patch (Unified Diff) or rewrite the entire file instead.
"@
    exit 1
}

Write-Host "agentrete: $cmd"
exit 0
