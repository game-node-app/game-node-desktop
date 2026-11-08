param([Parameter(Mandatory = $true)][string]$ExecutablePath)
$ErrorActionPreference = 'Stop'
if (!(Test-Path -LiteralPath $ExecutablePath -PathType Leaf)) {
    throw "Development executable not found: $ExecutablePath"
}
$plugin = Join-Path (Split-Path -Parent $ExecutablePath) 'gamenode-plugin-steam.exe'
if (!(Test-Path -LiteralPath $plugin -PathType Leaf)) {
    throw "Development Steam plugin not found beside the desktop: $plugin"
}
$process = Start-Process -FilePath $ExecutablePath -WorkingDirectory (Split-Path -Parent $ExecutablePath) -PassThru
$process.WaitForExit()
exit $process.ExitCode
