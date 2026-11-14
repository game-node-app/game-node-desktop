param([Parameter(Mandatory = $true)][string]$ExecutablePath)
$ErrorActionPreference = 'Stop'
if (!(Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "Development executable not found: $ExecutablePath"
}
$directory = Split-Path -Parent $ExecutablePath
$indexPath = Join-Path $directory 'bundled-plugins.json'
if (!(Test-Path -LiteralPath $indexPath -PathType Leaf)) {
    throw "Bundled plugin index not found beside the development executable: $indexPath. Run scripts/dev-windows-wsl.sh to rebuild and regenerate it."
}
$index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
if ($index.schema -ne 1 -or $null -eq $index.plugins) {
    throw "Bundled plugin index has an unsupported format: $indexPath"
}
foreach ($plugin in @($index.plugins)) {
    if ([string]::IsNullOrWhiteSpace($plugin.file)) {
        throw "Bundled plugin index contains an empty file name: $indexPath"
    }
    $pluginPath = Join-Path $directory $plugin.file
    if (!(Test-Path -LiteralPath $pluginPath -PathType Leaf)) {
        throw "Development plugin not found beside the desktop: $pluginPath"
    }
}
$process = Start-Process -FilePath $ExecutablePath -WorkingDirectory $directory -PassThru
$process.WaitForExit()
exit $process.ExitCode
