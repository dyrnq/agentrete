# agentrete shared PowerShell helpers
# Dot-source: . "$PSScriptRoot\_json_extract.ps1"

function json_val {
    param($body, $path, $default = "")
    try {
        $obj = $body | ConvertFrom-Json
        $parts = $path -split '\.'
        foreach ($p in $parts) {
            if ($p -match '^\d+$' -and $obj -is [array]) {
                $obj = $obj[[int]$p]
            } else {
                $obj = $obj.$p
            }
        }
        if ($null -ne $obj) { return $obj } else { return $default }
    } catch { return $default }
}

function json_lines {
    param($body, $path)
    try {
        $obj = $body | ConvertFrom-Json
        $parts = $path -split '\.'
        foreach ($p in $parts) {
            if ($p -match '^\d+$' -and $obj -is [array]) {
                $obj = $obj[[int]$p]
            } else {
                $obj = $obj.$p
            }
        }
        $arr = if ($obj -is [array]) { $obj } else { @($obj) }
        foreach ($x in $arr) {
            if ($x -is [string]) { $x } else { $x | ConvertTo-Json -Depth 2 -Compress }
        }
    } catch {}
}

function mcp_post {
    param($url, $jsonBody)
    try {
        $result = Invoke-RestMethod -Uri $url -Method Post -Body $jsonBody -ContentType "application/json" -TimeoutSec 5
        return ($result | ConvertTo-Json -Depth 6 -Compress)
    } catch { return "" }
}

function detect_project {
    try {
        $repo = git rev-parse --show-toplevel 2>$null
        if ($repo) { return (Split-Path -Leaf $repo) }
    } catch {}
    return (Split-Path -Leaf (Get-Location))
}
