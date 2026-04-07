param(
    [switch]$KeepArtifacts
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $repoRoot "target\release\harmony-mcp.exe"
if (-not (Test-Path $exe)) {
    throw "Missing release binary: $exe"
}

$root = Join-Path $env:TEMP ("harmony-network-smoke-" + [guid]::NewGuid().ToString())
$hostRoot = Join-Path $root "host"
$clientRoot = Join-Path $root "client"
$bridgeRoot = Join-Path $root "bridge"
$null = New-Item -ItemType Directory -Path $hostRoot, $clientRoot, $bridgeRoot -Force

$basePort = Get-Random -Minimum 54000 -Maximum 54500
$hostMcpPort = $basePort
$hostIpcPort = $basePort + 1
$hostWebPort = $basePort + 2
$clientIpcPort = $basePort + 3
$bridgeMcpPort = $basePort + 10
$bridgeIpcPort = $basePort + 11
$bridgeWebPort = $basePort + 12

$hostDb = Join-Path $hostRoot ".harmony\memory.db"
$hostConfigDir = Split-Path $hostDb -Parent
$clientConfigDir = Join-Path $clientRoot ".harmony"
$bridgeConfigDir = Join-Path $bridgeRoot ".harmony"
$null = New-Item -ItemType Directory -Path $hostConfigDir, $clientConfigDir, $bridgeConfigDir -Force

Set-Content -Path (Join-Path $bridgeConfigDir "config.toml") -Value @"
[human]
username = 'BridgeHost'
actor_id = 'human:bridge-host'

[network]
mode = 'host'
mcp_port = $bridgeMcpPort
ipc_port = $bridgeIpcPort
web_port = $bridgeWebPort
"@

function Wait-Url {
    param(
        [string]$Url,
        [int]$TimeoutSeconds = 20
    )

    $deadline = (Get-Date).ToUniversalTime().AddSeconds($TimeoutSeconds)
    while ((Get-Date).ToUniversalTime() -lt $deadline) {
        try {
            return Invoke-RestMethod -Uri $Url -TimeoutSec 2
        } catch {
            Start-Sleep -Milliseconds 250
        }
    }

    throw "Timed out waiting for $Url"
}

function Send-McpJsonLine {
    param(
        [int]$Port,
        [hashtable]$Payload
    )

    $client = [System.Net.Sockets.TcpClient]::new()
    $client.Connect("127.0.0.1", $Port)

    try {
        $stream = $client.GetStream()
        $writer = [System.IO.StreamWriter]::new($stream)
        $writer.AutoFlush = $true
        $reader = [System.IO.StreamReader]::new($stream)
        $json = $Payload | ConvertTo-Json -Compress -Depth 20
        $writer.WriteLine($json)
        $line = $reader.ReadLine()
        if ([string]::IsNullOrWhiteSpace($line)) {
            throw "No MCP response from port $Port"
        }

        return $line | ConvertFrom-Json
    } finally {
        $client.Dispose()
    }
}

function Read-WsEvents {
    param(
        [System.Net.WebSockets.ClientWebSocket]$WebSocket,
        [int]$TimeoutSeconds = 5
    )

    $messages = @()
    $buffer = New-Object byte[] 8192
    $deadline = (Get-Date).ToUniversalTime().AddSeconds($TimeoutSeconds)

    while ((Get-Date).ToUniversalTime() -lt $deadline) {
        $segment = [System.ArraySegment[byte]]::new($buffer)
        $task = $WebSocket.ReceiveAsync($segment, [System.Threading.CancellationToken]::None)
        if (-not $task.Wait(1000)) {
            continue
        }

        $result = $task.Result
        if ($result.MessageType -eq [System.Net.WebSockets.WebSocketMessageType]::Close) {
            break
        }

        $text = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $result.Count)
        if (-not [string]::IsNullOrWhiteSpace($text)) {
            $messages += $text
            if ($messages.Count -ge 3) {
                break
            }
        }
    }

    return $messages
}

$hostOut = Join-Path $root "host-stdout.log"
$hostErr = Join-Path $root "host-stderr.log"
$clientOut = Join-Path $root "client-stdout.log"
$clientErr = Join-Path $root "client-stderr.log"
$hostProc = $null
$clientProc = $null
$ws = $null

try {
    $hostProc = Start-Process -FilePath $exe -ArgumentList @(
        "--mode", "host",
        "--project-root", $hostRoot,
        "--db-path", $hostDb,
        "--mcp-port", $hostMcpPort,
        "--ipc-port", $hostIpcPort,
        "--web-port", $hostWebPort,
        "--host-name", "SmokeHost"
    ) -PassThru -RedirectStandardOutput $hostOut -RedirectStandardError $hostErr

    $null = Wait-Url -Url "http://127.0.0.1:$hostWebPort/api/status"

    $ws = [System.Net.WebSockets.ClientWebSocket]::new()
    $null = $ws.ConnectAsync(
        [uri]"ws://127.0.0.1:$hostWebPort/ws",
        [System.Threading.CancellationToken]::None
    ).GetAwaiter().GetResult()

    $clientProc = Start-Process -FilePath $exe -ArgumentList @(
        "--mode", "client",
        "--project-root", $clientRoot,
        "--host-url", "http://127.0.0.1:$hostMcpPort",
        "--ipc-port", $clientIpcPort,
        "--host-name", "SmokeClient"
    ) -PassThru -RedirectStandardOutput $clientOut -RedirectStandardError $clientErr

    $deadline = (Get-Date).ToUniversalTime().AddSeconds(15)
    $clientSeen = $false
    do {
        $status = Invoke-RestMethod -Uri "http://127.0.0.1:$hostWebPort/api/status" -TimeoutSec 2
        $clientSeen = @($status.connected_machines | Where-Object { $_.name -eq "SmokeClient" }).Count -gt 0
        if ($clientSeen) { break }
        Start-Sleep -Milliseconds 500
    } while ((Get-Date).ToUniversalTime() -lt $deadline)

    if (-not $clientSeen) {
        throw "Client machine never registered with host. Host stderr: $(Get-Content $hostErr -Raw) Client stderr: $(Get-Content $clientErr -Raw)"
    }

    $toolsList = Send-McpJsonLine -Port $clientIpcPort -Payload @{
        jsonrpc = "2.0"
        id = 1
        method = "tools/list"
        params = @{}
    }
    if (-not (($toolsList.result.tools | ForEach-Object { $_.name }) -contains "harmony_pulse")) {
        throw "Client proxy tools/list did not include harmony_pulse"
    }

    $null = Send-McpJsonLine -Port $hostIpcPort -Payload @{
        jsonrpc = "2.0"
        id = 2
        method = "tools/call"
        params = @{
            name = "report_change"
            arguments = @{
                actor_id = "agent:coder"
                file_path = "src/shared.ts"
                diff_unified = "@@ -1,0 +1,2 @@`n+host change`n+shared"
                start_line = 0
                end_line = 1
                task_prompt = "host edit"
            }
        }
    }

    $null = Send-McpJsonLine -Port $clientIpcPort -Payload @{
        jsonrpc = "2.0"
        id = 3
        method = "tools/call"
        params = @{
            name = "report_change"
            arguments = @{
                actor_id = "agent:coder"
                file_path = "src/shared.ts"
                diff_unified = "@@ -1,0 +1,2 @@`n+client change`n+shared"
                start_line = 0
                end_line = 1
                task_prompt = "client edit"
            }
        }
    }

    Start-Sleep -Milliseconds 1200

    $pulse = Send-McpJsonLine -Port $hostIpcPort -Payload @{
        jsonrpc = "2.0"
        id = 4
        method = "tools/call"
        params = @{
            name = "harmony_pulse"
            arguments = @{}
        }
    }
    $pulseText = $pulse.result.content[0].text
    if ($pulseText -notmatch "Pending overlaps: 1") {
        throw "Pulse did not report the expected overlap. Pulse text: $pulseText"
    }

    $overlapsResponse = Invoke-RestMethod -Uri "http://127.0.0.1:$hostWebPort/api/overlaps" -TimeoutSec 3
    if (@($overlapsResponse.overlaps).Count -lt 1) {
        throw "Dashboard API returned no overlaps after host/client changes"
    }

    $overlap = $overlapsResponse.overlaps[0]
    $machinesInOverlap = @($overlap.change_a.machine_name, $overlap.change_b.machine_name)
    if (($machinesInOverlap -notcontains "SmokeHost") -or ($machinesInOverlap -notcontains "SmokeClient")) {
        throw "Overlap did not preserve both machine names: $($machinesInOverlap -join ', ')"
    }

    $wsMessages = Read-WsEvents -WebSocket $ws -TimeoutSeconds 5
    if (-not (($wsMessages -match '"type":"log"') -or ($wsMessages -match '"type":"overlap"'))) {
        throw "WebSocket did not receive live events. Messages: $($wsMessages -join ' | ')"
    }

    $resolveBody = @{ overlap_id = $overlap.id; resolution = "accept_a" } | ConvertTo-Json -Compress
    $null = Invoke-RestMethod -Method Post -Uri "http://127.0.0.1:$hostWebPort/api/resolve" -ContentType "application/json" -Body $resolveBody -TimeoutSec 3
    $afterResolve = Invoke-RestMethod -Uri "http://127.0.0.1:$hostWebPort/api/overlaps" -TimeoutSec 3
    $resolvedOverlap = @($afterResolve.overlaps | Where-Object { $_.id -eq $overlap.id })[0]
    if (($resolvedOverlap.status | ConvertTo-Json -Compress) -eq '"pending"') {
        throw "Overlap resolution API did not persist the resolved status"
    }

    $bridgeOutput = @(
        '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1.0"}}}',
        '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}',
        '{"jsonrpc":"2.0","id":3,"method":"shutdown","params":{}}'
    ) | & $exe --stdio-bridge --project-root $bridgeRoot --db-path (Join-Path $bridgeConfigDir "memory.db")

    $bridgeText = ($bridgeOutput | Out-String)
    if ($bridgeText -notmatch "2025-06-18" -or $bridgeText -notmatch "harmony_pulse") {
        throw "stdio bridge smoke failed. Output: $bridgeText"
    }

    [pscustomobject]@{
        TempRoot = $root
        HostMcpPort = $hostMcpPort
        HostWebPort = $hostWebPort
        ClientIpcPort = $clientIpcPort
        Pulse = $pulseText
        WebSocketEvents = $wsMessages.Count
        ResolvedStatus = ($resolvedOverlap.status | ConvertTo-Json -Compress)
        BridgeSmoke = "ok"
    } | ConvertTo-Json -Compress -Depth 10
}
finally {
    if ($ws) {
        $ws.Dispose()
    }
    if ($clientProc) {
        try {
            Stop-Process -Id $clientProc.Id -Force -ErrorAction SilentlyContinue
            Wait-Process -Id $clientProc.Id -ErrorAction SilentlyContinue
        } catch {}
    }
    if ($hostProc) {
        try {
            Stop-Process -Id $hostProc.Id -Force -ErrorAction SilentlyContinue
            Wait-Process -Id $hostProc.Id -ErrorAction SilentlyContinue
        } catch {}
    }
    if (-not $KeepArtifacts -and (Test-Path $root)) {
        Start-Sleep -Milliseconds 500
        Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    }
}
