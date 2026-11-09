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
foreach ($name in @('gamenode.exe', 'gamenode-plugin-steam.exe', 'LICENSE', 'README.md', 'licenses', 'backup-assets', 'install.ps1', 'uninstall.ps1', 'previous-uri-handler.reg', '.gamenode-desktop-install.json')) {
    $path = Join-Path $installDir $name
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Recurse -Force }
}
if (!(Get-ChildItem -LiteralPath $installDir -Force)) { Remove-Item -LiteralPath $installDir }
Write-Output 'Removed GameNode Desktop integration. User data was preserved.'
