param(
    [Parameter(Mandatory = $true)][int]$Port,
    [Parameter(Mandatory = $true)][string]$Token,
    [Parameter(Mandatory = $true)][string]$Out,
    [Parameter(Mandatory = $true)][string]$Summary,
    [string]$StopFile = '',
    [double]$MaxHours = 26
)

# Bounded OneBot forward-WS event consumer for the N5 soak.
#
# It connects to /event, counts messages/bytes, reconnects on close, and writes
# only metadata (no message bodies). It is a test consumer, not a production
# client: it does not issue actions, does not replay unknown requests, and does
# not claim event delivery for a disconnected window.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$started = [DateTime]::UtcNow
$deadline = $started.AddHours($MaxHours)
$connects = 0
$messages = 0
$bytes = 0
$disconnects = 0
$errors = 0
$stopReason = 'deadline'
$writer = New-Object IO.StreamWriter($Out, $true, [Text.UTF8Encoding]::new($false))
$writer.AutoFlush = $true
$errorLog = "$Summary.err.log"

function Save-Summary {
    $value = [ordered]@{
        schema = 'caligo/n5-ws-drain/1'
        port = $Port
        started_utc = $started.ToString('o')
        finished_utc = [DateTime]::UtcNow.ToString('o')
        connects = $connects
        messages = $messages
        bytes = $bytes
        disconnects = $disconnects
        errors = $errors
        stop_reason = $stopReason
    }
    [IO.File]::WriteAllText($Summary, ($value | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
}

try {
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($StopFile -and (Test-Path -LiteralPath $StopFile)) { $stopReason = 'stop-file'; break }
        $ws = [Net.WebSockets.ClientWebSocket]::new()
        try {
            $ws.Options.SetRequestHeader('Authorization', "Bearer $Token")
            $ws.Options.KeepAliveInterval = [TimeSpan]::FromSeconds(20)
            $uri = [Uri]("ws://127.0.0.1:$Port/event")
            $cts = [Threading.CancellationTokenSource]::new(10000)
            [void]$ws.ConnectAsync($uri, $cts.Token).GetAwaiter().GetResult()
            $connects++
            $buffer = New-Object byte[] 65536
            $segment = [ArraySegment[byte]]::new($buffer)
            while ($true) {
                if ($StopFile -and (Test-Path -LiteralPath $StopFile)) { $stopReason = 'stop-file'; break }
                # A short cancellation aborts ClientWebSocket and forces endless
                # reconnects. 30s only bounds idle waits; continuous events return
                # immediately, and the stop file is checked after each result.
                $cts = [Threading.CancellationTokenSource]::new(30000)
                try {
                    $result = $ws.ReceiveAsync($segment, $cts.Token).GetAwaiter().GetResult()
                } catch [System.OperationCanceledException] {
                    if ($ws.State -ne [Net.WebSockets.WebSocketState]::Open) { break }
                    continue
                } catch {
                    $errors++
                    Add-Content -LiteralPath $errorLog -Value ("receive: " + $_.Exception.Message)
                    break
                }
                if ($result.MessageType -eq [Net.WebSockets.WebSocketMessageType]::Close) { break }
                if ($result.Count -gt 0) {
                    $bytes += $result.Count
                }
                if ($result.EndOfMessage) {
                    $messages++
                    $writer.WriteLine((@{ t_utc = [DateTime]::UtcNow.ToString('o'); bytes = $result.Count } | ConvertTo-Json -Compress))
                }
            }
        } catch {
            $errors++
            Add-Content -LiteralPath $errorLog -Value ("connect: " + $_.Exception.Message)
        } finally {
            if ($ws) {
                try { $ws.Dispose() } catch { }
            }
        }
        $disconnects++
        Start-Sleep -Milliseconds 500
    }
} finally {
    Save-Summary
    $writer.Dispose()
}

Write-Output ('ws-drain messages={0} bytes={1} connects={2} disconnects={3} errors={4} stop={5}' -f $messages, $bytes, $connects, $disconnects, $errors, $stopReason)
