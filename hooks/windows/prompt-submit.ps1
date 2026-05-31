# agentrete prompt-submit hook (Windows PowerShell)

$AGENTRETE_URL = if ($env:AGENTRETE_URL) { $env:AGENTRETE_URL } else { "http://127.0.0.1:9092" }

$input = $input | Out-String
if (-not $input) { exit 0 }
$query = $input.Substring(0, [Math]::Min(120, $input.Length))

$body = @{
    jsonrpc = "2.0"
    id = 1
    method = "tools/call"
    params = @{
        name = "memory_search"
        arguments = @{ query = $query; limit = 3 }
    }
} | ConvertTo-Json -Depth 4

try {
    $result = Invoke-RestMethod -Uri $AGENTRETE_URL -Method Post -Body $body -ContentType "application/json" -TimeoutSec 3
    if ($result.result.content) {
        Write-Host "agentrete recall:" -ForegroundColor Cyan
        foreach ($c in $result.result.content) {
            $text = if ($c.text.Length -gt 200) { $c.text.Substring(0, 200) } else { $c.text }
            Write-Host "  $text" -ForegroundColor Gray
        }
    }
} catch {}
exit 0
