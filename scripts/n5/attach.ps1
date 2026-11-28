param(
    [string]$Pack = $PSScriptRoot
)

# One admitted N5 attach. Requires the preflight state and the daemon startup
# record written by n5_soak.py. It refuses a reused generation, a changed
# module, an already mapped bridge, or a second reservation.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Load-Json([string]$Path) {
    [IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8) | ConvertFrom-Json
}

$state = Load-Json (Join-Path $Pack 'state.private.json')
$daemon = Load-Json (Join-Path $Pack 'daemon.private.json')
$exe = $state.build.caligo_exe
$bridge = $state.build.bridge

if (-not (Test-Path -LiteralPath $exe) -or -not (Test-Path -LiteralPath $bridge)) { throw 'Pinned release binaries are missing' }
if ((Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ne $state.build.caligo_sha256) { throw 'Operator EXE pin mismatch' }
if ((Get-FileHash -LiteralPath $bridge -Algorithm SHA256).Hash -ne $state.build.bridge_sha256) { throw 'Bridge pin mismatch' }
if ((Get-FileHash -LiteralPath $state.manifest -Algorithm SHA256).Hash -ne $state.manifest_sha256) { throw 'Manifest pin mismatch' }

$p = Get-Process -Id $state.pid -ErrorAction Stop
if ($p.StartTime.ToUniversalTime().ToString('o') -ne $state.created -or -not $p.Responding) {
    throw 'Target generation or health changed since preflight'
}
if (@($p.Modules | Where-Object ModuleName -like '*caligo*bridge*').Count -gt 0) { throw 'Bridge already mapped' }
foreach ($module in @($state.modules)) {
    $loaded = @($p.Modules | Where-Object ModuleName -eq $module.name)
    if ($loaded.Count -ne 1 -or
        [IO.Path]::GetFullPath($loaded[0].FileName) -ne [IO.Path]::GetFullPath([string]$module.path) -or
        (Get-FileHash -LiteralPath $loaded[0].FileName -Algorithm SHA256).Hash -ne $module.sha256) {
        throw "Loaded module changed: $($module.name)"
    }
}

$reservation = Join-Path $Pack 'attach-reserved'
if (Test-Path -LiteralPath $reservation) { throw 'Attach was already reserved for this pack; do not reattach' }
$handle = [IO.File]::Open($reservation, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
$handle.Dispose()

$generation = 1
if ($null -ne $state.session_generation) { $generation = [int]$state.session_generation }
if ($generation -lt 1) { throw 'State session_generation must be positive' }

$report = Join-Path $Pack 'entry.private.jsonl'
& $exe cli qq-entry --pid $state.pid --created-filetime $state.filetime `
    --bridge $bridge --manifest $state.manifest --report $report --wait-s 20 `
    --confirm-designated-test-instance --daemon-pipe $daemon.bridge_pipe `
    --daemon-auth-file $state.token_file --daemon-generation $generation --daemon-account ([string]$state.account) *> (Join-Path $Pack 'attach.private.log')
if ($LASTEXITCODE -ne 0) { throw 'Attach failed; no retry' }

$p.Refresh()
$mapped = @($p.Modules | Where-Object ModuleName -like '*caligo*bridge*')
if ($mapped.Count -ne 1) { throw 'Bridge mapping not observed after attach' }
if ([IO.Path]::GetFullPath($mapped[0].FileName) -ne [IO.Path]::GetFullPath($bridge)) {
    throw 'A different bridge was mapped'
}
Write-Output 'PASS one admitted attach; bridge mapped'
