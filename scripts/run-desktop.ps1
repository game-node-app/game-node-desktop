[CmdletBinding()]
param(
    [string[]]$CargoArguments = @(),
    [string[]]$AppArguments = @()
)
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
try {
    if ($CargoArguments -contains '--target' -or ($CargoArguments | Where-Object { $_ -like '--target=*' })) {
        throw 'run-desktop.ps1 builds for the current Windows host; use the cross-build workflow for other targets.'
    }
    & cargo build --locked --bins -p gamenode-app -p gamenode-plugin-steam @CargoArguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $metadata = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $targetDirectory = ($metadata | ConvertFrom-Json).target_directory
    $profile = if ($CargoArguments -contains '--release') { 'release' } else { 'debug' }
    $binaryDirectory = Join-Path $targetDirectory $profile
    $desktop = Join-Path $binaryDirectory 'gamenode.exe'
    $plugin = Join-Path $binaryDirectory 'gamenode-plugin-steam.exe'
    if (!(Test-Path -LiteralPath $desktop -PathType Leaf)) {
        throw "Desktop executable was not built: $desktop"
    }
    if (!(Test-Path -LiteralPath $plugin -PathType Leaf)) {
        throw "Steam plugin was not built beside the desktop: $plugin"
    }

    & $desktop @AppArguments
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
