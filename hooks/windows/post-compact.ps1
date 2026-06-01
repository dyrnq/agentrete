# agentrete PostCompact hook — reload context after compaction (Windows)
$AGENTRETE_URL = if ($env:AGENTRETE_URL) { $env:AGENTRETE_URL } else { "http://127.0.0.1:9092" }
$project = (Split-Path -Leaf (Get-Location))

# Show memory stats
$statsBody = @{
    jsonrpc = "2.0"; id = 1; method = "tools/call"
    params = @{ name = "memory_stats"; arguments = @{} }
} | ConvertTo-Json -Depth 3
try {
    $stats = Invoke-RestMethod -Uri $AGENTRETE_URL -Method Post -Body $statsBody -ContentType "application/json" -TimeoutSec 3
    if ($stats.result.content) { Write-Host "agentrete: $($stats.result.content[0].text)" -ForegroundColor Cyan }
} catch {}

# Reload project context
$searchBody = @{
    jsonrpc = "2.0"; id = 1; method = "tools/call"
    params = @{ name = "memory_search"; arguments = @{ query = $project; limit = 5 } }
} | ConvertTo-Json -Depth 4
try {
    $result = Invoke-RestMethod -Uri $AGENTRETE_URL -Method Post -Body $searchBody -ContentType "application/json" -TimeoutSec 3
    if ($result.result.content) {
        Write-Host "project context: $project" -ForegroundColor Cyan
        foreach ($c in $result.result.content) { Write-Host "  $($c.text)" -ForegroundColor Gray }
    }
} catch {}
exit 0
