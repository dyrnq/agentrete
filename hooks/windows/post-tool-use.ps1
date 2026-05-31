# agentrete post-tool-use hook (Windows PowerShell)

$AGENTRETE_URL = if ($env:AGENTRETE_URL) { $env:AGENTRETE_URL } else { "http://127.0.0.1:9092" }

$input = $input | Out-String
if (-not $input) { exit 0 }

$tool = ""
try {
    $data = $input | ConvertFrom-Json
    $tool = if ($data.tool_name) { $data.tool_name } elseif ($data.tool) { $data.tool } else { "" }
} catch {}

if (-not $tool) { exit 0 }

$writeTools = @("Edit", "Write", "Bash", "exec_command", "apply_patch", "Delete")
if ($writeTools -contains $tool) {
    $body = @{
        jsonrpc = "2.0"
        id = 1
        method = "tools/call"
        params = @{
            name = "memory_save"
            arguments = @{ content = "Tool call: $tool"; type = "fact"; tags = "hook,tool-call" }
        }
    } | ConvertTo-Json -Depth 4
    try { Invoke-RestMethod -Uri $AGENTRETE_URL -Method Post -Body $body -ContentType "application/json" -TimeoutSec 3 | Out-Null } catch {}
}
exit 0
