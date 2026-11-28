[CmdletBinding()]
param(
    [string]$Repo = '',
    [string]$TargetDir = 'target-harness',
    [string]$OutDir = '',
    [string]$Manifest = '',
    [switch]$SkipBuild,
    [switch]$AllowDirty
)

# N5 startup-package and unknown-version gate.
#
# This script is offline: it builds the default release entry, packages it with
# the same script CI uses, verifies the package hashes/build-info, starts the
# packaged daemon twice on the same journal/token and checks honest OneBot
# responses, then proves a tampered module manifest is rejected before any
# bridge load or report write. It does not touch a running QQ process.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

if (-not $Repo) { $Repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path }
$Repo = (Resolve-Path -LiteralPath $Repo).Path
$target = 'x86_64-pc-windows-msvc'
$version = $null
$commit = $null
$started = [DateTime]::UtcNow
if (-not $OutDir) { $OutDir = Join-Path $Repo "$TargetDir/n5-offline-gate" }
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$OutDir = (Resolve-Path -LiteralPath $OutDir).Path

$steps = New-Object System.Collections.ArrayList
$summary = [ordered]@{
    schema = 'caligo/n5-offline-gate/1'
    started_utc = $started.ToString('o')
    repo = $Repo
    target = $target
    overall = 'running'
    steps = $steps
    failure = $null
}
$exitCode = 0

function Add-Step {
    param([string]$Name, [string]$Status, $Detail)
    [void]$steps.Add([ordered]@{
        name = $Name
        status = $Status
        detail = $Detail
        at_utc = [DateTime]::UtcNow.ToString('o')
    })
    Write-Host ("[{0}] {1}" -f $Status.ToUpperInvariant(), $Name)
}

function Save-Summary {
    $summary.finished_utc = [DateTime]::UtcNow.ToString('o')
    $path = Join-Path $OutDir 'offline-gate.json'
    [IO.File]::WriteAllText($path, ($summary | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))
    Write-Host "summary=$path"
}

function Get-FreePort {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try { return ([Net.IPEndPoint]$listener.LocalEndpoint).Port } finally { $listener.Stop() }
}

function New-Hex {
    param([int]$Bytes)
    $builder = New-Object Text.StringBuilder
    for ($i = 0; $i -lt $Bytes; $i++) { [void]$builder.Append(('{0:x2}' -f (Get-Random -Maximum 256))) }
    return $builder.ToString()
}

function Read-FirstJsonLine {
    param([string]$Path, [int]$TimeoutMs = 15000)
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $Path) {
            $lines = @(Get-Content -LiteralPath $Path -Encoding UTF8 -TotalCount 1 -ErrorAction SilentlyContinue)
            if ($lines.Count -gt 0 -and $lines[0]) {
                try { return ($lines[0] | ConvertFrom-Json) } catch { }
            }
        }
        Start-Sleep -Milliseconds 100
    }
    return $null
}

function Invoke-ExeLogged {
    param([string]$Exe, [string[]]$Arguments, [string]$Stdout, [string]$Stderr)
    $proc = Start-Process -FilePath $Exe -ArgumentList $Arguments -NoNewWindow -PassThru -Wait `
        -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
    return $proc.ExitCode
}

function Invoke-OneBot {
    param([int]$Port, [string]$Token, [string]$Action, $Params = @{})
    $body = [ordered]@{ action = $Action; params = $Params; echo = $Action } | ConvertTo-Json -Depth 8 -Compress
    $headers = @{ Authorization = "Bearer $Token" }
    return Invoke-RestMethod -Uri ("http://127.0.0.1:{0}/" -f $Port) -Method Post -Body $body `
        -ContentType 'application/json' -Headers $headers -TimeoutSec 10
}

function Start-GateDaemon {
    param(
        [string]$Exe,
        [string]$ExtractDir,
        [string]$PipePrefix,
        [string]$Journal,
        [string]$TokenFile,
        [int]$HttpPort,
        [int]$WsPort,
        [string]$Account,
        [string]$Stdout,
        [string]$Stderr
    )
    Remove-Item -LiteralPath $Stdout, $Stderr -Force -ErrorAction SilentlyContinue
    $arguments = @(
        'daemon',
        '--pipe-prefix', $PipePrefix,
        '--journal', $Journal,
        '--baseline', 'local-pilot-2026-11-07-qq-9.9.36-53644',
        '--account', $Account,
        '--core-build', 'caligo-n5-offline-gate',
        '--token-file', $TokenFile,
        '--onebot-http-bind', ('127.0.0.1:{0}' -f $HttpPort),
        '--onebot-ws-bind', ('127.0.0.1:{0}' -f $WsPort)
    )
    $proc = Start-Process -FilePath $Exe -ArgumentList $arguments -WorkingDirectory $ExtractDir `
        -PassThru -WindowStyle Hidden -RedirectStandardOutput $Stdout -RedirectStandardError $Stderr
    $ready = Read-FirstJsonLine -Path $Stdout -TimeoutMs 20000
    if (-not $ready) {
        if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
        $errorText = if (Test-Path $Stderr) { [IO.File]::ReadAllText($Stderr) } else { '' }
        throw "daemon did not print its ready line: $errorText"
    }
    if (-not $ready.onebot_http -or -not $ready.onebot_ws) { throw 'daemon ready line is missing OneBot listeners' }
    return [pscustomobject]@{ process = $proc; ready = $ready }
}

function Stop-GateDaemon {
    param([string]$Exe, $Ready, $Process)
    $code = Invoke-ExeLogged -Exe $Exe -Arguments @(
        'cli', 'daemon-control', '--pipe', $Ready.control_pipe, '--auth', $Ready.auth_token, 'stop'
    ) -Stdout (Join-Path $OutDir 'control-stop.stdout.private.log') -Stderr (Join-Path $OutDir 'control-stop.stderr.private.log')
    $exited = $false
    for ($i = 0; $i -lt 200; $i++) {
        if ($Process.HasExited) { $exited = $true; break }
        Start-Sleep -Milliseconds 50
    }
    if (-not $exited) {
        Stop-Process -Id $Process.Id -Force -ErrorAction SilentlyContinue
        throw 'daemon did not exit after control Stop'
    }
    if ($code -ne 0) { throw "daemon-control stop exited $code" }
    return $code
}

try {
    Push-Location -LiteralPath $Repo
    try {
        $version = [regex]::Match([IO.File]::ReadAllText((Join-Path $Repo 'Cargo.toml')), '(?ms)^\[workspace\.package\]\s*\r?\nversion\s*=\s*"([^"]+)"').Groups[1].Value
        $commit = (& git rev-parse HEAD).Trim()
        $dirtyLines = @(& git status --porcelain | Where-Object { $_ -notmatch '^\?\?' })
        $sourceDirty = $dirtyLines.Count -gt 0
        if ($sourceDirty -and -not $AllowDirty) {
            throw "tracked working tree is dirty ($($dirtyLines.Count) entries); commit or pass -AllowDirty for a rehearsal"
        }
        $summary.version = $version
        $summary.commit = $commit
        $summary.source_dirty = $sourceDirty
        $summary.rustc = (& rustc --version).Trim()
        Add-Step 'source-identity' 'pass' ([ordered]@{ version = $version; commit = $commit; source_dirty = $sourceDirty })
    } finally { Pop-Location }

    if (-not $Manifest) {
        $localPilot = Join-Path $Repo 'local-evidence/n4-r2-r3-closure-20261107/manifest-qq-9.9.36-53644-pilot.json'
        $Manifest = if (Test-Path -LiteralPath $localPilot) { $localPilot } else { Join-Path $Repo 'docs/contracts/version-adapter-manifest.json' }
    }
    $Manifest = (Resolve-Path -LiteralPath $Manifest).Path
    $summary.manifest = [ordered]@{
        path = $Manifest
        sha256 = (Get-FileHash -LiteralPath $Manifest -Algorithm SHA256).Hash
    }
    Add-Step 'manifest-source' 'pass' $summary.manifest

    $binaryRoot = Join-Path $Repo (Join-Path $TargetDir (Join-Path $target 'release'))
    if (-not $SkipBuild) {
        Add-Step 'release-build' 'running' $null
        $previousTarget = $env:CARGO_BUILD_TARGET
        $env:CARGO_BUILD_TARGET = $target
        $buildOut = Join-Path $OutDir 'release-build.stdout.private.log'
        $buildErr = Join-Path $OutDir 'release-build.stderr.private.log'
        try {
            $build = Start-Process -FilePath 'cargo' -ArgumentList @(
                'build', '--release', '--locked', '--offline', '--target-dir', $TargetDir,
                '-p', 'caligo-cli', '--bin', 'caligo', '-p', 'caligo-bridge', '--lib'
            ) -NoNewWindow -PassThru -Wait -RedirectStandardOutput $buildOut -RedirectStandardError $buildErr
            if ($build.ExitCode -ne 0) {
                $tail = if (Test-Path $buildErr) { Get-Content $buildErr -Tail 20 | Out-String } else { '' }
                throw "cargo build exited $($build.ExitCode): $tail"
            }
        } finally {
            if ($null -eq $previousTarget) { Remove-Item Env:CARGO_BUILD_TARGET -ErrorAction SilentlyContinue }
            else { $env:CARGO_BUILD_TARGET = $previousTarget }
        }
        ($steps[$steps.Count - 1]).status = 'pass'
        Write-Host '[PASS] release-build'
    }
    $exe = Join-Path $binaryRoot 'caligo.exe'
    $bridge = Join-Path $binaryRoot 'caligo_bridge.dll'
    if (-not (Test-Path -LiteralPath $exe) -or -not (Test-Path -LiteralPath $bridge)) {
        throw "release binaries missing under $binaryRoot"
    }
    $summary.build = [ordered]@{
        caligo_exe = $exe
        caligo_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
        bridge = $bridge
        bridge_sha256 = (Get-FileHash -LiteralPath $bridge -Algorithm SHA256).Hash
    }
    Add-Step 'release-binaries' 'pass' $summary.build

    Add-Step 'package-release' 'running' $null
    $packageOut = Join-Path $OutDir 'package-release.stdout.private.log'
    $packageErr = Join-Path $OutDir 'package-release.stderr.private.log'
    $package = Start-Process -FilePath 'C:\windows\System32\WindowsPowerShell\v1.0\powershell.exe' -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
        (Join-Path $Repo '.github/scripts/package-release.ps1')
    ) -NoNewWindow -PassThru -Wait -RedirectStandardOutput $packageOut -RedirectStandardError $packageErr
    if ($package.ExitCode -ne 0) {
        $tail = if (Test-Path $packageErr) { Get-Content $packageErr -Tail 20 | Out-String } else { '' }
        throw "package-release.ps1 exited $($package.ExitCode): $tail"
    }
    ($steps[$steps.Count - 1]).status = 'pass'
    Write-Host '[PASS] package-release'

    $archive = Join-Path $Repo (Join-Path $TargetDir ('dist/caligo-{0}-windows-x64.zip' -f $version))
    $sidecar = "$archive.sha256"
    if (-not (Test-Path -LiteralPath $archive) -or -not (Test-Path -LiteralPath $sidecar)) {
        throw "release archive or sidecar missing: $archive"
    }
    $archiveHash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    $sidecarHash = ([IO.File]::ReadAllText($sidecar)).Split(' ')[0].Trim().ToLowerInvariant()
    if ($archiveHash -ne $sidecarHash) { throw 'archive SHA-256 does not match its sidecar' }
    $summary.package = [ordered]@{
        archive = $archive
        sha256 = $archiveHash
        sidecar = $sidecar
        size_bytes = (Get-Item -LiteralPath $archive).Length
    }
    Add-Step 'package-checksum' 'pass' $summary.package

    $extract = Join-Path $OutDir 'package'
    if (Test-Path -LiteralPath $extract) { Remove-Item -LiteralPath $extract -Recurse -Force }
    Expand-Archive -LiteralPath $archive -DestinationPath $extract -Force
    $infoPath = Join-Path $extract 'build-info.json'
    if (-not (Test-Path -LiteralPath $infoPath)) { throw 'build-info.json missing from the package' }
    $info = [IO.File]::ReadAllText($infoPath) | ConvertFrom-Json
    if ($info.version -ne $version) { throw "build-info version $($info.version) != $version" }
    if ($info.commit -ne $commit) { throw "build-info commit $($info.commit) != $commit" }
    if ($info.target -ne $target) { throw "build-info target $($info.target) != $target" }
    if ($info.mode -ne 'default' -or $info.research -ne $false) { throw 'build-info is not a default/non-research build' }
    if ($info.entrypoint -ne 'caligo.exe') { throw "build-info entrypoint $($info.entrypoint) != caligo.exe" }
    $expectedNames = @('LICENSE', 'README.md', 'build-info.json', 'caligo.exe', 'caligo_bridge.dll')
    $actualNames = @(Get-ChildItem -LiteralPath $extract -File | ForEach-Object Name | Sort-Object)
    $diff = @(Compare-Object -ReferenceObject $expectedNames -DifferenceObject $actualNames)
    if ($diff.Count -ne 0) { throw "package file allowlist mismatch: $($diff | ForEach-Object { $_.InputObject })" }
    foreach ($entry in @($info.files)) {
        $path = Join-Path $extract $entry.name
        if (-not (Test-Path -LiteralPath $path)) { throw "build-info lists missing file $($entry.name)" }
        $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($hash -ne $entry.sha256) { throw "package file hash mismatch for $($entry.name)" }
    }
    $summary.package.extracted = $extract
    $summary.package.build_info = $info
    Add-Step 'package-integrity' 'pass' ([ordered]@{ files = $expectedNames; build_info_files = @($info.files).Count })

    Add-Step 'package-startup' 'running' $null
    $smokeRoot = Join-Path $OutDir 'smoke'
    New-Item -ItemType Directory -Force -Path $smokeRoot | Out-Null
    $journal = Join-Path $smokeRoot 'journal.jsonl'
    Remove-Item -LiteralPath $journal -Force -ErrorAction SilentlyContinue
    $tokenFile = Join-Path $smokeRoot 'auth.token'
    $token = New-Hex 32
    [IO.File]::WriteAllText($tokenFile, $token, [Text.Encoding]::ASCII)
    $pipePrefix = 'caligo-n5-gate-' + (New-Hex 6)
    $httpPort = Get-FreePort
    $wsPort = Get-FreePort
    $daemon = Start-GateDaemon -Exe (Join-Path $extract 'caligo.exe') -ExtractDir $extract `
        -PipePrefix $pipePrefix -Journal $journal -TokenFile $tokenFile `
        -HttpPort $httpPort -WsPort $wsPort -Account '1694717255' `
        -Stdout (Join-Path $smokeRoot 'daemon-1.stdout.private.log') `
        -Stderr (Join-Path $smokeRoot 'daemon-1.stderr.private.log')
    $versionProbe = Invoke-OneBot -Port $httpPort -Token $token -Action 'get_version_info'
    if ($versionProbe.retcode -ne 0 -or -not $versionProbe.data) { throw 'packaged daemon did not answer get_version_info' }
    $loginProbe = Invoke-OneBot -Port $httpPort -Token $token -Action 'get_login_info'
    if ($loginProbe.retcode -eq 0) { throw 'packaged daemon invented a login without a bridge' }
    Stop-GateDaemon -Exe (Join-Path $extract 'caligo.exe') -Ready $daemon.ready -Process $daemon.process | Out-Null
    $journalBefore = if (Test-Path -LiteralPath $journal) { (Get-FileHash -LiteralPath $journal -Algorithm SHA256).Hash } else { '' }
    $journalSizeBefore = if (Test-Path -LiteralPath $journal) { (Get-Item -LiteralPath $journal).Length } else { 0 }

    $httpPort2 = Get-FreePort
    $wsPort2 = Get-FreePort
    $daemon2 = Start-GateDaemon -Exe (Join-Path $extract 'caligo.exe') -ExtractDir $extract `
        -PipePrefix $pipePrefix -Journal $journal -TokenFile $tokenFile `
        -HttpPort $httpPort2 -WsPort $wsPort2 -Account '1694717255' `
        -Stdout (Join-Path $smokeRoot 'daemon-2.stdout.private.log') `
        -Stderr (Join-Path $smokeRoot 'daemon-2.stderr.private.log')
    $versionProbe2 = Invoke-OneBot -Port $httpPort2 -Token $token -Action 'get_version_info'
    if ($versionProbe2.retcode -ne 0 -or -not $versionProbe2.data) { throw 'restarted packaged daemon did not answer get_version_info' }
    Stop-GateDaemon -Exe (Join-Path $extract 'caligo.exe') -Ready $daemon2.ready -Process $daemon2.process | Out-Null
    $journalAfter = if (Test-Path -LiteralPath $journal) { (Get-FileHash -LiteralPath $journal -Algorithm SHA256).Hash } else { '' }
    $journalSizeAfter = if (Test-Path -LiteralPath $journal) { (Get-Item -LiteralPath $journal).Length } else { 0 }
    if ($journalSizeAfter -lt $journalSizeBefore) { throw 'journal shrank across packaged restart' }
    if ($journalSizeBefore -gt 0) {
        $beforeBytes = [IO.File]::ReadAllBytes($journal)
        $afterBytes = [IO.File]::ReadAllBytes($journal)
        for ($i = 0; $i -lt $beforeBytes.Length; $i++) {
            if ($beforeBytes[$i] -ne $afterBytes[$i]) { throw 'journal prefix changed across packaged restart' }
        }
    }
    ($steps[$steps.Count - 1]).status = 'pass'
    $summary.startup = [ordered]@{
        package_exe = (Join-Path $extract 'caligo.exe')
        first_http = $httpPort
        second_http = $httpPort2
        version_retcode = $versionProbe.retcode
        login_without_bridge_retcode = $loginProbe.retcode
        journal_before = $journalBefore
        journal_after = $journalAfter
        journal_size_before = $journalSizeBefore
        journal_size_after = $journalSizeAfter
    }
    Write-Host ('[PASS] package-startup (version={0}, login_without_bridge={1})' -f $versionProbe.retcode, $loginProbe.retcode)

    Add-Step 'unknown-version-rejection' 'running' $null
    $manifestJson = [IO.File]::ReadAllText($Manifest, [Text.Encoding]::UTF8) | ConvertFrom-Json
    $module = @($manifestJson.modules | Where-Object hash_checked)[0]
    if (-not $module) { throw 'manifest has no hash-checked module to tamper' }
    $originalHash = [string]$module.sha256
    $tamperedHash = if ($originalHash.Substring(0, 1) -eq 'A') { 'B' + $originalHash.Substring(1) } else { 'A' + $originalHash.Substring(1) }
    $module.sha256 = $tamperedHash
    $tamperedManifest = Join-Path $OutDir 'manifest-tampered.private.json'
    [IO.File]::WriteAllText($tamperedManifest, ($manifestJson | ConvertTo-Json -Depth 12), [Text.UTF8Encoding]::new($false))

    $packageExe = Join-Path $extract 'caligo.exe'
    $verifyOut = Join-Path $OutDir 'verify-tampered.stdout.private.log'
    $verifyErr = Join-Path $OutDir 'verify-tampered.stderr.private.log'
    $verifyCode = Invoke-ExeLogged -Exe $packageExe -Arguments @('cli', 'verify-manifest', $tamperedManifest) -Stdout $verifyOut -Stderr $verifyErr
    if ($verifyCode -ne 2) { throw "verify-manifest on a tampered manifest exited $verifyCode (expected 2)" }
    if (-not (Select-String -LiteralPath $verifyOut -Pattern 'RESULT: REJECT' -Quiet)) { throw 'tampered manifest did not print RESULT: REJECT' }

    $report = Join-Path $OutDir 'entry-tampered.private.jsonl'
    Remove-Item -LiteralPath $report -Force -ErrorAction SilentlyContinue
    $daemonAuth = Join-Path $OutDir 'gate-auth.private.token'
    [IO.File]::WriteAllText($daemonAuth, (New-Hex 32), [Text.Encoding]::ASCII)
    $entryBridge = Join-Path $extract 'caligo_bridge.dll'
    $entryOut = Join-Path $OutDir 'entry-tampered.stdout.private.log'
    $entryErr = Join-Path $OutDir 'entry-tampered.stderr.private.log'
    $entryCode = Invoke-ExeLogged -Exe $packageExe -Arguments @(
        'cli', 'qq-entry', '--pid', '999999', '--created-filetime', '1',
        '--bridge', $entryBridge, '--manifest', $tamperedManifest, '--report', $report, '--wait-s', '1',
        '--confirm-designated-test-instance', '--daemon-pipe', 'caligo-n5-gate-offline',
        '--daemon-auth-file', $daemonAuth, '--daemon-generation', '1', '--daemon-account', '1694717255'
    ) -Stdout $entryOut -Stderr $entryErr
    if ($entryCode -ne 2) { throw "qq-entry on a tampered manifest exited $entryCode (expected 2)" }
    if (-not (Select-String -LiteralPath $entryErr -Pattern '\[gate 1\] REJECT' -Quiet)) { throw 'qq-entry did not reject at gate 1' }
    if (Test-Path -LiteralPath $report) { throw 'qq-entry wrote a report despite rejecting the manifest' }

    $injectReport = Join-Path $OutDir 'inject-tampered.private.json'
    Remove-Item -LiteralPath $injectReport -Force -ErrorAction SilentlyContinue
    $injectOut = Join-Path $OutDir 'inject-tampered.stdout.private.log'
    $injectErr = Join-Path $OutDir 'inject-tampered.stderr.private.log'
    $injectCode = Invoke-ExeLogged -Exe $packageExe -Arguments @(
        'cli', 'inject', '--pid', '999999', '--bridge', $entryBridge,
        '--manifest', $tamperedManifest, '--report', $injectReport, '--confirm-designated-test-instance'
    ) -Stdout $injectOut -Stderr $injectErr
    # The default release disables the legacy inject/exec route before argument
    # or manifest processing (K4-D0). Exit 3 with no report is the expected
    # zero-native-call result; the manifest gate itself is proven by qq-entry.
    if ($injectCode -ne 3) { throw "inject on the default release exited $injectCode (expected 3: legacy route disabled)" }
    if (-not (Select-String -LiteralPath $injectErr -Pattern 'K4-D0' -Quiet)) {
        throw 'inject did not report the default-build legacy-route refusal'
    }
    if (Test-Path -LiteralPath $injectReport) { throw 'inject wrote a report despite the default-build refusal' }
    ($steps[$steps.Count - 1]).status = 'pass'
    $summary.unknown_version = [ordered]@{
        manifest = $tamperedManifest
        tampered_module = $module.path
        original_sha256 = $originalHash
        tampered_sha256 = $tamperedHash
        verify_manifest_exit = $verifyCode
        qq_entry_exit = $entryCode
        qq_entry_report_absent = -not (Test-Path -LiteralPath $report)
        inject_exit = $injectCode
        inject_report_absent = -not (Test-Path -LiteralPath $injectReport)
    }
    Write-Host '[PASS] unknown-version-rejection (zero bridge load/report write)'

    $summary.overall = 'pass'
    Add-Step 'overall' 'pass' $null
} catch {
    $summary.overall = 'fail'
    $summary.failure = $_.Exception.Message
    Add-Step 'overall' 'fail' $summary.failure
    Write-Error -Message $_.Exception.Message -ErrorAction Continue
    $exitCode = 1
} finally {
    Save-Summary
}

exit $exitCode
