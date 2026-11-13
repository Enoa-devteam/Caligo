[CmdletBinding()]
param(
    [ValidateSet('docs', 'default', 'research', 'all')]
    [string]$Mode = 'docs'
)

$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    Write-Host "Running: $Program $($Arguments -join ' ')"
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Program failed with exit code $LASTEXITCODE"
    }
}

Push-Location -LiteralPath $PSScriptRoot
try {
    Invoke-Checked -Program 'node' -Arguments @('scripts/check-docs.mjs')
    if ($Mode -ne 'docs') {
        Invoke-Checked -Program 'cargo' -Arguments @('--version')
        if ($Mode -in @('default', 'all')) {
            Invoke-Checked -Program 'cargo' -Arguments @(
                'test', '--workspace', '--locked', '--offline', '--target-dir', 'target-harness'
            )
        }
        if ($Mode -in @('research', 'all')) {
            Invoke-Checked -Program 'cargo' -Arguments @(
                'test', '--workspace', '--features', 'caligo-core/research,caligo-cli/research',
                '--locked', '--offline', '--target-dir', 'target-harness'
            )
        }
    }
    Write-Host "Verification complete ($Mode). QQ field acceptance is separate."
}
catch {
    Write-Error -Message $_ -ErrorAction Continue
    exit 1
}
finally {
    Pop-Location
}
