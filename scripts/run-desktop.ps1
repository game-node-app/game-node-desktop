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

    # The plugin executable set comes from the trusted descriptors. PowerShell cannot read TOML,
    # so the Python helper prints the cargo arguments and file names.
    $python = $null
    $pythonPrefix = @()
    foreach ($candidate in @(@('py', '-3'), @('python'))) {
        if (!(Get-Command $candidate[0] -ErrorAction SilentlyContinue)) { continue }
        $prefix = @($candidate | Select-Object -Skip 1)
        $version = & $candidate[0] @prefix --version 2>$null
        if ($LASTEXITCODE -eq 0 -and $version -match '^Python (\d+)\.(\d+)' -and
            ([int]$Matches[1] -gt 3 -or ([int]$Matches[1] -eq 3 -and [int]$Matches[2] -ge 11))) {
            $python = $candidate[0]
            $pythonPrefix = $prefix
            break
        }
    }
    if ($null -eq $python) {
        throw 'Python 3.11+ is required to discover the bundled plugin executables from plugins/*.toml. Install Python 3.11+ and ensure `py -3` or `python` is on PATH.'
    }
    $helper = Join-Path $PSScriptRoot 'bundled_plugins.py'
    $pluginCargoArguments = @(& $python @pythonPrefix $helper --platform windows --format cargo-args)
    if ($LASTEXITCODE -ne 0) { throw "Could not read the bundled plugin descriptors with $helper" }
    $pluginFiles = @(& $python @pythonPrefix $helper --platform windows --format files)
    if ($LASTEXITCODE -ne 0) { throw "Could not read the bundled plugin descriptors with $helper" }
    if ($pluginFiles.Count -eq 0) { throw 'No bundled plugins are declared for Windows.' }

    & cargo build --locked --bins -p gamenode-app @CargoArguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    & cargo build --locked @pluginCargoArguments @CargoArguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $metadata = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    $targetDirectory = ($metadata | ConvertFrom-Json).target_directory
    $buildProfile = if ($CargoArguments -contains '--release') { 'release' } else { 'debug' }
    $binaryDirectory = Join-Path $targetDirectory $buildProfile
    $desktop = Join-Path $binaryDirectory 'gamenode.exe'
    if (!(Test-Path -LiteralPath $desktop -PathType Leaf)) {
        throw "Desktop executable was not built: $desktop"
    }
    foreach ($pluginFile in $pluginFiles) {
        $plugin = Join-Path $binaryDirectory $pluginFile
        if (!(Test-Path -LiteralPath $plugin -PathType Leaf)) {
            throw "Provider plugin was not built beside the desktop: $plugin"
        }
    }

    & $desktop @AppArguments
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
