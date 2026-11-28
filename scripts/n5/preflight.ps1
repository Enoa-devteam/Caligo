[CmdletBinding()]
param(
    [string]$Repo = '',
    [string]$Pack = '',
    [UInt32]$TargetPid = 0,
    [string]$ExpectedCreated = '',
    [UInt64]$Account = 0,
    [double]$DurationHours = 24,
    [int]$SampleSeconds = 60,
    [int[]]$RestartAtMinutes = @(120, 480, 960),
    [int]$Generation = 1,
    [switch]$RequireEventConsumer,
    [switch]$Rehearse
)

# N5 field admission and pack preparation.
#
# Field mode requires a fresh, designated QQ main root. It performs the same
# read-only gates used by the feat-006 pack: exact release pins, manifest and
# loaded-module hashes, PE/backend audit, unique Environment scan, no mapped
# Caligo bridge, and unique loopback ports. It writes private state only under
# the ignored pack directory. Rehearse mode writes a synthetic pack state for
# the offline soak rehearsal and never inspects QQ.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $Repo) { $Repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path }
$Repo = (Resolve-Path -LiteralPath $Repo).Path
if (-not $Pack) { $Pack = Join-Path $Repo 'local-evidence/feat007-n5-20261125-unbound' }
New-Item -ItemType Directory -Force -Path $Pack | Out-Null
$Pack = (Resolve-Path -LiteralPath $Pack).Path

$exe = Join-Path $Repo 'target-harness/x86_64-pc-windows-msvc/release/caligo.exe'
$bridge = Join-Path $Repo 'target-harness/x86_64-pc-windows-msvc/release/caligo_bridge.dll'
$manifest = Join-Path $Repo 'local-evidence/n4-r2-r3-closure-20261107/manifest-qq-9.9.36-53644-pilot.json'
$audit = Join-Path $Repo 'local-evidence/n4-r2-callback-20261108/audit-g3-target-modules.ps1'
$prior = Join-Path $Repo 'local-evidence/snowcat-dual-login-fresh-20261114/state.private.json'

function Save-Json([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, ($Value | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
}
function New-Hex([int]$Bytes) {
    $builder = New-Object Text.StringBuilder
    for ($i = 0; $i -lt $Bytes; $i++) { [void]$builder.Append(('{0:x2}' -f (Get-Random -Maximum 256))) }
    return $builder.ToString()
}
function Assert-FreePort([int]$Port) {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, $Port)
    try { $listener.Start() } catch { throw "Port $Port is already in use" } finally { $listener.Stop() }
}
function Get-FreePort {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try { return ([Net.IPEndPoint]$listener.LocalEndpoint).Port } finally { $listener.Stop() }
}

if ($Generation -lt 1) { throw '-Generation must be a positive session generation' }
if (-not (Test-Path -LiteralPath $exe) -or -not (Test-Path -LiteralPath $bridge)) {
    throw 'Release binaries missing; run scripts/n5/verify-release.ps1 first'
}
if (-not (Test-Path -LiteralPath $manifest)) { throw 'Field manifest missing' }

$state = [ordered]@{
    schema = 'caligo/n5-soak-state/1'
    mode = if ($Rehearse) { 'rehearsal' } else { 'field' }
    source_head = (& git -C $Repo rev-parse --short HEAD).Trim()
    prepared_utc = [DateTime]::UtcNow.ToString('o')
    build = [ordered]@{
        caligo_exe = $exe
        caligo_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
        bridge = $bridge
        bridge_sha256 = (Get-FileHash -LiteralPath $bridge -Algorithm SHA256).Hash
    }
    manifest = $manifest
    manifest_sha256 = (Get-FileHash -LiteralPath $manifest -Algorithm SHA256).Hash
    account = $Account
    session_generation = $Generation
    pid = $TargetPid
    created = $ExpectedCreated
    filetime = 0
    env = '0x0'
    modules = @()
    pipe = 'caligo-n5-' + (New-Hex 6)
    journal = Join-Path $Pack 'field/journal.jsonl'
    token_file = Join-Path $Pack 'onebot.token'
    ports = [ordered]@{ http = 0; ws = 0 }
    duration_hours = $DurationHours
    sample_seconds = $SampleSeconds
    restart_at_minutes = @($RestartAtMinutes)
    require_event_consumer = [bool]$RequireEventConsumer
}

if ($Rehearse) {
    $state.pid = 0
    $state.created = ''
    $state.account = 1694717255
    $state.ports.http = 0
    $state.ports.ws = 0
    $state.journal = Join-Path $Pack 'rehearsal/journal.jsonl'
    New-Item -ItemType Directory -Force -Path (Split-Path $state.journal) | Out-Null
} else {
    if (-not $TargetPid -or -not $ExpectedCreated) { throw '-TargetPid and -ExpectedCreated are required in field mode' }
    $p = Get-Process -Id $TargetPid -ErrorAction Stop
    if ($p.StartTime.ToUniversalTime().ToString('o') -ne $ExpectedCreated) { throw 'Target creation time mismatch' }
    if (-not $p.Responding) { throw 'Target is not responding' }
    $mods = @($p.Modules | ForEach-Object {
        [pscustomobject]@{ name = $_.ModuleName; path = $_.FileName; size = $_.ModuleMemorySize }
    })
    if (@($mods | Where-Object name -like '*caligo*bridge*').Count -gt 0) { throw 'Bridge already mapped; use a fresh QQ generation' }
    if (@($mods | Where-Object name -eq 'wrapper.node').Count -ne 1 -or @($mods | Where-Object name -eq 'major.node').Count -ne 1) {
        throw 'Target is not the QQ main root (major.node/wrapper.node not both loaded)'
    }
    if ($p.MainWindowHandle -eq 0) { Write-Warning 'Target has no visible main window; continuing because module/hash/admission checks pass' }

    & $exe cli verify-manifest $manifest *> (Join-Path $Pack 'manifest.private.log')
    if ($LASTEXITCODE -ne 0) { throw 'Manifest rejected' }
    $pm = [IO.File]::ReadAllText($manifest, [Text.Encoding]::UTF8) | ConvertFrom-Json
    $verified = @()
    foreach ($name in @('QQNT.dll', 'major.node', 'wrapper.node')) {
        $loaded = @($mods | Where-Object name -eq $name)
        $pin = @($pm.modules | Where-Object { [IO.Path]::GetFileName([string]$_.path) -eq $name })
        if ($loaded.Count -ne 1 -or $pin.Count -ne 1) { throw "Loaded module count mismatch $name" }
        $hash = (Get-FileHash -LiteralPath $loaded[0].path -Algorithm SHA256).Hash
        if ($hash -ne $pin[0].sha256) { throw "Loaded module mismatch $name" }
        $verified += [pscustomobject]@{ name = $name; path = $loaded[0].path; sha256 = $pin[0].sha256 }
    }

    $auditPath = Join-Path $Pack 'audit.private.json'
    $auditor = Start-Process 'C:\windows\System32\WindowsPowerShell\v1.0\powershell.exe' -WindowStyle Hidden -PassThru -Wait `
        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $audit,
            '-ProcessId', $TargetPid, '-ManifestPath', $manifest, '-ReportPath', $auditPath) `
        -RedirectStandardOutput (Join-Path $Pack 'audit.stdout.private.log') `
        -RedirectStandardError (Join-Path $Pack 'audit.stderr.private.log')
    if ($auditor.ExitCode -ne 0) { throw "Backend audit exited $($auditor.ExitCode)" }
    $auditResult = [IO.File]::ReadAllText($auditPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    if ($auditResult.decision -ne 'PASS_KNOWN_BACKEND_AND_TEMP_MODULE_GATE') {
        throw "Backend audit rejected: $($auditResult.decision)"
    }

    $scanPath = Join-Path $Pack 'envscan.private.json'
    & $exe cli envscan --pid $TargetPid --vtable 0xA804990 --context 0 --out $scanPath *> (Join-Path $Pack 'envscan.private.log')
    if ($LASTEXITCODE -ne 0) { throw 'Read-only Environment scan failed' }
    $scan = [IO.File]::ReadAllText($scanPath, [Text.Encoding]::UTF8) | ConvertFrom-Json
    if ($scan.pid -ne $TargetPid -or @($scan.hits).Count -ne 1 -or $scan.regions_read_failed -ne 0) {
        throw 'Environment scan was not unique and clean'
    }

    $targets = ([IO.File]::ReadAllText($prior, [Text.Encoding]::UTF8) | ConvertFrom-Json).targets
    if (-not $Account) { $Account = [UInt64]($targets | Where-Object alias -eq 'B').account }
    $registered = @($targets | Where-Object { [UInt64]$_.account -eq $Account })
    if ($registered.Count -ne 1) { throw 'Account is not one of the registered test targets' }

    $state.pid = $TargetPid
    $state.created = $ExpectedCreated
    $state.filetime = $p.StartTime.ToFileTimeUtc()
    $state.account = $Account
    $state.env = ('0x{0:x}' -f [UInt64]$scan.hits[0].candidate_env)
    $state.modules = $verified
    $state.ports.http = Get-FreePort
    $state.ports.ws = Get-FreePort
    Assert-FreePort $state.ports.http
    Assert-FreePort $state.ports.ws
    Write-Output ('PASS designated main root pid={0}; exact manifest/modules; PE/backend audit; unique read-only Environment' -f $TargetPid)
}

New-Item -ItemType Directory -Force -Path (Split-Path $state.journal) | Out-Null
if (-not $Rehearse) {
    New-Hex 32 | Set-Content -LiteralPath $state.token_file -NoNewline -Encoding Ascii
}
Save-Json (Join-Path $Pack 'state.private.json') $state

$authorization = [ordered]@{
    authorized = $false
    pid = $state.pid
    created_utc = $state.created
    scope = 'One admitted attach; zero message sends; resident OneBot queries for the soak duration; up to the listed core crash/restart injections; bridge retained at the end (no hot unload).'
    allow_core_restart = $true
    sends = 0
    note = 'Operator sets authorized=true only after reviewing this pack and the designated QQ main root. No message is sent by the harness.'
}
Save-Json (Join-Path $Pack 'authorization.template.json') $authorization

$preparation = [ordered]@{
    schema = 'caligo/n5-preparation/1'
    scope = 'issue #16 criterion 2/4: one fresh QQ generation, one admitted attach, 24h single-account resident probes, controlled core crash/restart injections, bounded bridge/daemon process metrics, honest stop boundary without hot unload.'
    mode = $state.mode
    source_head = $state.source_head
    build = $state.build
    manifest_sha256 = $state.manifest_sha256
    ports = $state.ports
    duration_hours = $state.duration_hours
    sample_seconds = $state.sample_seconds
    restart_at_minutes = $state.restart_at_minutes
    require_event_consumer = $state.require_event_consumer
    session_generation = $state.session_generation
    expected = [ordered]@{
        resident = 'one daemon and one admitted bridge; OneBot get_version_info/get_status/get_login_info continue to answer without per-message CLI interaction'
        restart = 'each injected core crash is followed by the same journal/pipe/token, bridge reconnects, and login/query becomes available again'
        journal = 'journal prefix is preserved; no request is replayed or duplicated; no message is sent'
        stop = 'control Stop ends the daemon; the default bridge has no owner-stop and stays mapped; QQ remains responsive'
    }
    refusals = @(
        'preflight state missing or bound to another PID/created time/build hash',
        'module, manifest, PE/backend or unique Environment check failed -> stop without attach',
        'authorization.private.json missing/mismatched/not authorized -> runner refuses before starting services',
        'any unresolved restart is never retried blindly; the bridge is not hot-unloaded',
        'never reattach to the consumed QQ generation'
    )
}
Save-Json (Join-Path $Pack 'preparation-unbound.json') $preparation

$files = @(Get-ChildItem -LiteralPath $Pack -File | Where-Object { $_.Name -notlike '*.private.*' -and $_.Name -notin @('onebot.token') } | Sort-Object Name)
$manifestEntries = @(foreach ($file in $files) {
    [ordered]@{ name = $file.Name; sha256 = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
})
$seal = [ordered]@{
    schema = 'caligo/n5-pack/1'
    prepared_utc = [DateTime]::UtcNow.ToString('o')
    source_head = $state.source_head
    files = $manifestEntries
}
Save-Json (Join-Path $Pack 'pack-manifest.json') $seal
$aggregate = [System.BitConverter]::ToString(([Security.Cryptography.SHA256]::Create().ComputeHash(
    [Text.Encoding]::UTF8.GetBytes(($manifestEntries | ConvertTo-Json -Depth 6 -Compress))))).Replace('-', '').ToLowerInvariant()
Write-Output ('pack={0} aggregate={1}' -f $Pack, $aggregate)
Write-Output ('ports http={0} ws={1}; journal={2}' -f $state.ports.http, $state.ports.ws, $state.journal)
