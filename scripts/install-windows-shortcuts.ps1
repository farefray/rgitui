# Re-run after moving the checkout. No admin rights required.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$launcher = Join-Path $PSScriptRoot 'run-windows.ps1'
$powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$shell = New-Object -ComObject WScript.Shell

foreach ($folder in @(
    [Environment]::GetFolderPath('Desktop'),
    [Environment]::GetFolderPath('Programs')
)) {
    if (-not $folder) {
        throw 'Windows could not locate the Desktop or Start Menu folder.'
    }
    $path = Join-Path $folder 'rgitui (latest).lnk'
    $shortcut = $shell.CreateShortcut($path)
    $shortcut.TargetPath = $powershell
    $shortcut.Arguments = '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File "' + $launcher + '" -ShowErrors'
    $shortcut.WorkingDirectory = $root
    $shortcut.IconLocation = (Join-Path $root 'assets\icons\app-icon.ico') + ',0'
    $shortcut.Description = 'Launch the newest local rgitui build (debug or release)'
    $shortcut.Save()
    Write-Host "Created $path"
}
