param(
    [switch]$UseProjectDb,
    [switch]$KeepArtifacts
)

$ErrorActionPreference = "Stop"

function Write-Step {
    param([string]$Message)
    Write-Host "[Harmony Demo] $Message"
}

function Remove-DbArtifacts {
    param([string]$DbPath)

    $dbDir = Split-Path -Parent $DbPath
    $dbName = Split-Path -Leaf $DbPath
    $artifactNames = @(
        $dbName,
        "$dbName-shm",
        "$dbName-wal"
    )

    foreach ($artifactName in $artifactNames) {
        $artifactPath = Join-Path $dbDir $artifactName
        if (Test-Path $artifactPath) {
            Remove-Item -LiteralPath $artifactPath -Force
        }
    }
}

function ConvertTo-JsonLine {
    param($Value)
    $Value | ConvertTo-Json -Compress -Depth 10
}

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$binaryPath = Join-Path $repoRoot "target\release\harmony-mcp.exe"
$harmonyDir = Join-Path $repoRoot ".harmony"
$requestLogPath = Join-Path $harmonyDir "demo-overlap-requests.jsonl"
$responseLogPath = Join-Path $harmonyDir "demo-overlap-responses.jsonl"

if (-not (Test-Path $binaryPath)) {
    throw "Missing $binaryPath. Build it first with: cargo build --release -p harmony-mcp"
}

New-Item -ItemType Directory -Force -Path $harmonyDir | Out-Null

$dbPath = if ($UseProjectDb) {
    Join-Path $harmonyDir "memory.db"
} else {
    Join-Path $harmonyDir "demo-overlap.db"
}

if (-not $UseProjectDb) {
    Write-Step "Resetting demo database at $dbPath"
    Remove-DbArtifacts -DbPath $dbPath
}

$requests = @(
    @{
        jsonrpc = "2.0"
        id = 0
        method = "initialize"
        params = @{
            protocolVersion = "2025-03-26"
            capabilities = @{}
            clientInfo = @{
                name = "Harmony overlap demo"
                version = "1.0.0"
            }
        }
    },
    @{
        jsonrpc = "2.0"
        method = "notifications/initialized"
        params = $null
    },
    @{
        jsonrpc = "2.0"
        id = 1
        method = "tools/call"
        params = @{
            name = "report_change"
            arguments = @{
                actor_id = "human:demo-user"
                file_path = "src/demo-overlap.ts"
                diff_unified = "@@ -10,4 +10,6 @@`n+const validationMode = 'strict';`n+const minScore = 75;"
                start_line = 10
                end_line = 18
                task_prompt = "Human updates validation rules"
            }
        }
    },
    @{
        jsonrpc = "2.0"
        id = 2
        method = "tools/call"
        params = @{
            name = "report_change"
            arguments = @{
                actor_id = "agent:demo-bot"
                file_path = "src/demo-overlap.ts"
                diff_unified = "@@ -14,3 +14,5 @@`n+const validationMode = 'balanced';`n+const autoApproveThreshold = 80;"
                start_line = 14
                end_line = 22
                task_prompt = "Agent refactors the same validation block"
            }
        }
    },
    @{
        jsonrpc = "2.0"
        id = 3
        method = "tools/call"
        params = @{
            name = "harmony_pulse"
            arguments = @{}
        }
    },
    @{
        jsonrpc = "2.0"
        id = 4
        method = "shutdown"
    }
)

$requestLines = $requests | ForEach-Object { ConvertTo-JsonLine $_ }
Set-Content -LiteralPath $requestLogPath -Value $requestLines -Encoding UTF8

Write-Step "Sending two overlapping changes into Harmony"
$responseLines = $requestLines | & $binaryPath --db-path $dbPath
Set-Content -LiteralPath $responseLogPath -Value $responseLines -Encoding UTF8

$responses = $responseLines |
    Where-Object { $_ -and $_.Trim().StartsWith("{") } |
    ForEach-Object { $_ | ConvertFrom-Json }

$pulseResponse = $responses | Where-Object { $_.id -eq 3 } | Select-Object -First 1
if (-not $pulseResponse) {
    throw "The demo did not receive a harmony_pulse response. Check $responseLogPath"
}

$pulseText = $pulseResponse.result.content[0].text
if (-not $pulseText) {
    throw "The demo received an empty harmony_pulse response. Check $responseLogPath"
}

if ($pulseText -notmatch "Pending overlaps:\s+1") {
    throw "Expected Harmony Pulse to report exactly 1 pending overlap, but got:`n`n$pulseText"
}

$cliPulse = & $binaryPath pulse --db-path $dbPath

Write-Host ""
Write-Step "Overlap demo succeeded."
Write-Host ""
Write-Host $pulseText
Write-Host ""
Write-Step "CLI verification:"
Write-Host $cliPulse
Write-Host ""
Write-Step "Artifacts:"
Write-Host "Database: $dbPath"
Write-Host "Requests: $requestLogPath"
Write-Host "Responses: $responseLogPath"

if (-not $UseProjectDb) {
    Write-Host ""
    Write-Step "This run used the isolated demo database, so your normal Zed project state was not modified."
    Write-Step "If you want Zed chat to show the same overlap, rerun with: .\test-harmony-overlap.ps1 -UseProjectDb"
}

if (-not $KeepArtifacts) {
    Write-Host ""
    Write-Step "Artifacts were kept so you can inspect the overlap database and JSON-RPC logs."
}
