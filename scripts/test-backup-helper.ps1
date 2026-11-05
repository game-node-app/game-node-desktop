# Isolated Windows contract smoke test. Never selects real game saves.
param([Parameter(Mandatory=$true)][string]$Helper)
$ErrorActionPreference = 'Stop'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('GameNode-helper-' + [Guid]::NewGuid())
$config = Join-Path $fixture 'config'
$archive = Join-Path $fixture 'archive'
$save = Join-Path $fixture 'custom.sav'
$title = 'GameNode isolated contract fixture'
$token = 'GameNodeFixture-' + [Guid]::NewGuid().ToString('N')
$local = Join-Path $env:LOCALAPPDATA $token
$documents = Join-Path ([Environment]::GetFolderPath('MyDocuments')) $token
$registry = 'HKCU:\Software\' + $token

function Invoke-Helper([string[]]$Arguments) {
    $output = & $Helper --config $config --no-manifest-update @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Helper failed ($LASTEXITCODE): $output" }
    return (($output -join "`n") | ConvertFrom-Json)
}
try {
    New-Item -ItemType Directory -Path $config,$local,$documents -Force | Out-Null
    New-Item -Path $registry -Force | Out-Null
    New-ItemProperty -Path $registry -Name Progress -Value 'original' -PropertyType String | Out-Null
    $base = Join-Path $fixture 'renamed install with spaces'
    New-Item -ItemType Directory -Path $base -Force | Out-Null
    $paths = @($save, (Join-Path $base 'progress.sav'), (Join-Path $local 'progress.sav'), (Join-Path $documents 'progress.sav'))
    foreach ($path in $paths) { [IO.File]::WriteAllText($path, 'original fixture save') }
    $rules = @{}
    # Bind the private definition's <base> to this exact renamed installation.
    $rules[('<base>/progress.sav'.Replace('<base>', $base.Replace('\','/')))] = @{}
    $rules['<winLocalAppData>/' + $token + '/progress.sav'] = @{}
    $rules['<winDocuments>/' + $token + '/progress.sav'] = @{}
    $rules[$save.Replace('\','/')] = @{}
    $manifest = @{}
    $manifest[$title] = @{ files = $rules; registry = @{ ('HKEY_CURRENT_USER/Software/' + $token) = @{} } }
    $manifest['GameNode empty fixture'] = @{ files = @{ ($fixture.Replace('\','/') + '/absent.sav') = @{} } }
    [IO.File]::WriteAllText((Join-Path $config 'manifest.yaml'), ($manifest | ConvertTo-Json -Depth 15))
    $configData = @{ roots = @(); manifest = @{ enable = $true; secondary = @() }; customGames = @() }
    [IO.File]::WriteAllText((Join-Path $config 'config.yaml'), ($configData | ConvertTo-Json -Depth 10))
    $empty = Invoke-Helper @('backup','--preview','--api','--no-cloud-sync','--path',$archive,'--','GameNode empty fixture')
    $preview = Invoke-Helper @('backup','--preview','--api','--no-cloud-sync','--path',$archive,'--',$title)
    if (-not $preview.games.$title) { throw 'Preview omitted fixture game' }
    if (@($preview.games.$title.files.PSObject.Properties).Count -ne 4) { throw 'Preview did not resolve all four isolated file rules' }
    if (@($preview.games.$title.registry.PSObject.Properties).Count -ne 1) { throw 'Preview omitted registry fixture' }
    $backup = Invoke-Helper @('backup','--force','--api','--no-cloud-sync','--path',$archive,'--full-limit','10','--differential-limit','0','--',$title)
    $history = Invoke-Helper @('backups','--api','--path',$archive,'--',$title)
    $id = $history.games.$title.backups[0].name
    if (-not $id) { throw 'History omitted backup id' }
    foreach ($path in $paths) { [IO.File]::WriteAllText($path, 'changed fixture save') }
    Set-ItemProperty -Path $registry -Name Progress -Value 'changed'
    $restorePreview = Invoke-Helper @('restore','--preview','--api','--no-cloud-sync','--path',$archive,'--backup',$id,'--',$title)
    if (-not $restorePreview.games.$title) { throw 'Restore preview omitted fixture' }
    $restored = Invoke-Helper @('restore','--force','--api','--no-cloud-sync','--path',$archive,'--backup',$id,'--',$title)
    foreach ($path in $paths) {
        if ([IO.File]::ReadAllText($path) -ne 'original fixture save') { throw "Selected backup did not restore $path" }
    }
    if ((Get-ItemProperty -Path $registry).Progress -ne 'original') { throw 'Registry value was not restored' }
    Write-Output 'PASS: exact renamed base, appdata, documents, custom file, registry, empty preview, history and selected restore'

} finally {
    foreach ($directory in @($local,$documents)) { if (Test-Path $directory) { Remove-Item -Recurse -Force $directory } }
    if (Test-Path $registry) { Remove-Item -Recurse -Force $registry }
    if (Test-Path $fixture) { Remove-Item -Recurse -Force $fixture }
}
