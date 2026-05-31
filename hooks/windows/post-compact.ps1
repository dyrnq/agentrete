# agentrete post-compact hook (Windows PowerShell)
$AGENTRETE_URL = if ($env:AGENTRETE_URL) { $env:AGENTRETE_URL } else { "http://127.0.0.1:9092" }
$project = (Split-Path -Leaf (Get-Location))
$body = @{
    jsonrpc = "2.0"; id = 1; method = "tools/call"
    params = @{ name = "memory_search"; arguments = @{ query = $project; limit = 5 } }
} | ConvertTo-Json -Depth 4
try {
    $result = Invoke-RestMethod -Uri $AGENTRETE_URL -Method Post -Body $body -ContentType "application/json" -TimeoutSec 3
    if ($result.result.content) {
        Write-Host "agentrete reload: $project" -ForegroundColor Cyan
        foreach ($c in $result.result.content) { Write-Host "  $($c.text)" -ForegroundColor Gray }
    }
} catch {}
exit 0
