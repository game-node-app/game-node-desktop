[CmdletBinding()]
param([string]$SourceDirectory = $PSScriptRoot)
$ErrorActionPreference = 'Stop'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\GameNode Desktop'
$marker = Join-Path $installDir '.gamenode-desktop-install.json'
$scheme = 'HKCU:\Software\Classes\app.gamenode'
$binary = Join-Path $installDir 'gamenode.exe'
$sourceBinary = Join-Path $SourceDirectory 'gamenode.exe'
if (!(Test-Path -LiteralPath $sourceBinary)) { throw 'Package does not contain gamenode.exe' }

# The package index is the only plugin list. Validate every indexed file before touching the
# existing installation, so a truncated package leaves the current installation unchanged.
$indexPath = Join-Path $SourceDirectory 'bundled-plugins.json'
if (!(Test-Path -LiteralPath $indexPath)) { throw 'Package does not contain bundled-plugins.json' }
$index = Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json
if ($index.schema -ne 1) { throw 'Package plugin index has an unsupported schema' }
$pluginFiles = @($index.plugins | ForEach-Object { [string]$_.file })
if ($pluginFiles.Count -eq 0) { throw 'Package plugin index does not list any plugins' }
foreach ($file in $pluginFiles) {
    if ($file -notmatch '^[A-Za-z0-9._-]+$') { throw "Package plugin index has an invalid file name: $file" }
    $sourcePlugin = Join-Path $SourceDirectory $file
    if (!(Test-Path -LiteralPath $sourcePlugin -PathType Leaf)) { throw "Package is missing indexed plugin: $file" }
}

if ((Test-Path -LiteralPath $installDir) -and !(Test-Path -LiteralPath $marker)) { throw 'Installation directory is not owned by GameNode Desktop' }
$previousPlugins = @()
if (Test-Path -LiteralPath $marker) {
    $owned = Get-Content -LiteralPath $marker -Raw | ConvertFrom-Json
    if ($owned.product -ne 'app.gamenode.desktop') { throw 'Installation marker is not owned by GameNode Desktop' }
    # Keep plugins installed by earlier packages in the record so uninstall can remove a plugin
    # that a later package dropped from its index.
    foreach ($file in @($owned.plugins)) {
        $file = [string]$file
        if (!$file) { continue }
        if ($file -notmatch '^[A-Za-z0-9._-]+$') { throw "Installation record has an invalid plugin file name: $file" }
        $previousPlugins += $file
    }
}
if (Test-Path $scheme) {
    $owner = (Get-ItemProperty $scheme -ErrorAction Stop).GameNodeDesktopOwner
    $preservePreviousHandler = $owner -ne $installDir
}
New-Item -ItemType Directory -Path $installDir -Force | Out-Null
if ($preservePreviousHandler) {
    $backup = Join-Path $installDir 'previous-uri-handler.reg'
    if (Test-Path -LiteralPath $backup) { throw 'Refusing to overwrite a previous URI handler backup' }
    & reg.exe export 'HKCU\Software\Classes\app.gamenode' $backup /y | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not preserve the previous URI handler' }
}
foreach ($name in @('gamenode.exe', 'bundled-plugins.json', 'LICENSE', 'README.md', 'licenses', 'backup-assets', 'install.ps1', 'uninstall.ps1')) {
    $source = Join-Path $SourceDirectory $name
    if (!(Test-Path -LiteralPath $source)) { throw "Package is missing $name" }
    Copy-Item -LiteralPath $source -Destination $installDir -Recurse -Force
}
foreach ($file in $pluginFiles) {
    Copy-Item -LiteralPath (Join-Path $SourceDirectory $file) -Destination $installDir -Force
}
$installedPlugins = @($pluginFiles + $previousPlugins | Select-Object -Unique)
@{ product = 'app.gamenode.desktop'; binary = $binary; plugins = @($installedPlugins) } | ConvertTo-Json | Set-Content -LiteralPath $marker -Encoding UTF8
New-Item -Path "$scheme\shell\open\command" -Force | Out-Null
Set-Item -Path $scheme -Value 'URL:GameNode Protocol'
New-ItemProperty -Path $scheme -Name 'URL Protocol' -Value '' -Force | Out-Null
New-ItemProperty -Path $scheme -Name 'GameNodeDesktopOwner' -Value $installDir -Force | Out-Null
Set-Item -Path "$scheme\shell\open\command" -Value ('"' + $binary + '" "%1"')
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'GameNode Desktop.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $binary
$shortcut.IconLocation = "$binary,0"
$shortcut.WorkingDirectory = $installDir
$shortcut.Description = 'GameNode Desktop'
$shortcut.Save()
Write-Output "Installed: $binary"
