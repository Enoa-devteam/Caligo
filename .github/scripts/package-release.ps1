[CmdletBinding()]
param()

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

# Explicit allowlist for the public package.
$sources = @(
    (Join-Path $binaryRoot 'caligo.exe')
    (Join-Path $binaryRoot 'caligo_bridge.dll')
    (Join-Path $repoRoot 'README.md')
    (Join-Path $repoRoot 'LICENSE')
)
$files = @(foreach ($source in $sources) {
    if (!(Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing release file: $source" }
    [ordered]@{
        name = Split-Path $source -Leaf
        sha256 = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
    }
})

$name = "caligo-$version-windows-x64"
$archive = Join-Path $dist "$name.zip"
$staging = Join-Path $dist ([guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $staging -Force | Out-Null
$buildInfo = Join-Path $staging 'build-info.json'
try {
    [ordered]@{
        version = $version
        commit = $commit
        toolchain = $toolchain
        target = $target
        mode = 'default'
        research = $false
        entrypoint = 'caligo.exe'
        acceptance = 'Offline build only; QQ field acceptance and production readiness are separate.'
        files = $files
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $buildInfo -Encoding utf8

    Compress-Archive -LiteralPath ($sources + @($buildInfo)) -DestinationPath $archive -Force
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $name.zip" | Set-Content -LiteralPath (Join-Path $dist "$name.zip.sha256") -Encoding ascii
} finally {
    if (Test-Path -LiteralPath $buildInfo) { Remove-Item -LiteralPath $buildInfo -Force }
    Remove-Item -LiteralPath $staging
}
Write-Host "Packaged $archive ($commit)"
