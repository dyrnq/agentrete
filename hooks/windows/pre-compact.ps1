# agentrete pre-compact hook (Windows PowerShell)
$AGENTRETE_URL = if ($env:AGENTRETE_URL) { $env:AGENTRETE_URL } else { "http://127.0.0.1:9092" }
$body = @{
    jsonrpc = "2.0"; id = 1; method = "tools/call"
    params = @{ name = "memory_save"; arguments = @{ content = "Context snapshot before compaction"; type = "fact"; tags = "hook,compact" } }
} | ConvertTo-Json -Depth 4
try { Invoke-RestMethod -Uri $AGENTRETE_URL -Method Post -Body $body -ContentType "application/json" -TimeoutSec 3 | Out-Null } catch {}
exit 0
