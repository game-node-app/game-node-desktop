[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\GameNode Desktop'
$marker = Join-Path $installDir '.gamenode-desktop-install.json'
if (!(Test-Path -LiteralPath $marker)) { throw 'GameNode Desktop installation marker not found' }
$owned = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
if ($owned.product -ne 'app.gamenode.desktop') { throw 'Installation is not owned by GameNode Desktop' }
$scheme = 'HKCU:\Software\Classes\app.gamenode'
if ((Test-Path $scheme) -and ((Get-ItemProperty $scheme).GameNodeDesktopOwner -eq $installDir)) {
    Remove-Item $scheme -Recurse
    $backup = Join-Path $installDir 'previous-uri-handler.reg'
    if (Test-Path -LiteralPath $backup) {
        & reg.exe import $backup | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'Could not restore the previous URI handler; backup retained' }
    }
}
Remove-ItemProperty -Path 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -Name 'GameNode Desktop' -ErrorAction SilentlyContinue
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'GameNode Desktop.lnk'
if (Test-Path -LiteralPath $shortcutPath) {
    $shell = New-Object -ComObject WScript.Shell
    if ($shell.CreateShortcut($shortcutPath).TargetPath -eq (Join-Path $installDir 'gamenode.exe')) { Remove-Item -LiteralPath $shortcutPath }
}

# Remove the union of what the installation recorded and what the current package indexed. An
# older marker without `plugins` contributes nothing, so the installed index still cleans up.
$pluginFiles = @()
foreach ($file in @($owned.plugins)) {
    $file = [string]$file
    if (!$file) { continue }
    if ($file -notmatch '^[A-Za-z0-9._-]+$') { throw "Installation record has an invalid plugin file name: $file" }
    $pluginFiles += $file
}
$indexPath = Join-Path $installDir 'bundled-plugins.json'
if (Test-Path -LiteralPath $indexPath) {
    $index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
    foreach ($plugin in @($index.plugins)) {
        $file = [string]$plugin.file
        if (!$file) { continue }
        if ($file -notmatch '^[A-Za-z0-9._-]+$') { throw "Installed plugin index has an invalid file name: $file" }
        $pluginFiles += $file
    }
}
$pluginFiles = @($pluginFiles | Select-Object -Unique)

foreach ($name in @('gamenode.exe', 'bundled-plugins.json', 'LICENSE', 'README.md', 'licenses', 'backup-assets', 'install.ps1', 'uninstall.ps1', 'previous-uri-handler.reg', '.gamenode-desktop-install.json')) {
    $path = Join-Path $installDir $name
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
}
foreach ($file in $pluginFiles) {
    $path = Join-Path $installDir $file
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
}
if (!(Get-ChildItem -LiteralPath $installDir -Force)) { Remove-Item -LiteralPath $installDir }
# Keep-data policy: user data and provider credentials (including the `provider.epic` Windows
# Credential Manager entry) are preserved. Uninstall adds no new data-destruction path.
Write-Output 'Removed GameNode Desktop integration. User data was preserved.'
