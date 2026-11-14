[CmdletBinding()]
param(
    [ValidateSet('default', 'research')]
    [string]$Mode = 'default'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$target = 'x86_64-pc-windows-msvc'
$binaryRoot = Join-Path $repoRoot "target-harness/$target/release"
$dist = Join-Path $repoRoot 'target-harness/dist'
$manifest = Get-Content (Join-Path $repoRoot 'Cargo.toml') -Raw
$version = [regex]::Match($manifest, '(?ms)^\[workspace\.package\]\s*\r?\nversion\s*=\s*"([^"]+)"').Groups[1].Value
if (!$version) { throw 'Missing workspace package version' }
$commit = & git -C $repoRoot rev-parse HEAD
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve source commit' }
$toolchain = & rustc --version
if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve Rust toolchain' }

$name = "caligo-$version-$Mode-$target"
$staging = Join-Path $dist $name
if (Test-Path $staging) { Remove-Item $staging -Recurse -Force }
New-Item -ItemType Directory -Path $staging -Force | Out-Null

# Explicit allowlist: never collect local evidence, QQ files, caches or credentials.
$binaries = @('caligo-cli.exe', 'caligod.exe', 'onebotd.exe', 'caligo-manager.exe', 'caligo-media.exe', 'caligo_bridge.dll')
foreach ($binary in $binaries) {
    $source = Join-Path $binaryRoot $binary
    if (!(Test-Path $source -PathType Leaf)) { throw "Missing release binary: $source" }
    Copy-Item -LiteralPath $source -Destination $staging
}
foreach ($file in @('README.md', 'LICENSE')) {
    Copy-Item -LiteralPath (Join-Path $repoRoot $file) -Destination $staging
}
$files = @(Get-ChildItem $staging -File | Sort-Object Name | ForEach-Object {
    [ordered]@{ name = $_.Name; sha256 = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
})
[ordered]@{
    version = $version
    commit = $commit
    toolchain = $toolchain
    target = $target
    mode = $Mode
    research = ($Mode -eq 'research')
    acceptance = 'Offline build only; QQ field acceptance and production readiness are separate.'
    files = $files
} | ConvertTo-Json -Depth 5 | Set-Content (Join-Path $staging 'build-info.json') -Encoding utf8
$archive = Join-Path $dist "$name.zip"
Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $archive -Force
$hash = (Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $name.zip" | Set-Content (Join-Path $dist "$name.zip.sha256") -Encoding ascii
Remove-Item $staging -Recurse -Force
Write-Host "Packaged $archive ($commit, $Mode)"
