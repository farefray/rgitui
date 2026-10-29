# Re-run after moving the checkout. No admin rights required.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$launchers = @(
    @{
        Name = 'rgitui (latest)'
        Script = 'run-windows.ps1'
        Options = '-WindowStyle Hidden '
        Description = 'Launch the newest local rgitui build (debug or release)'
    },
    @{
        Name = 'rgitui (build & run)'
        Script = 'build-run-windows.ps1'
        Options = ''
        Description = 'Build the current local source, then launch rgitui (close the app first)'
    }
)
$powershell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
$shell = New-Object -ComObject WScript.Shell

foreach ($folder in @(
    [Environment]::GetFolderPath('Desktop'),
    [Environment]::GetFolderPath('Programs')
)) {
    if (-not $folder) {
        throw 'Windows could not locate the Desktop or Start Menu folder.'
    }
    foreach ($launcher in $launchers) {
        $path = Join-Path $folder ($launcher.Name + '.lnk')
        $script = Join-Path $PSScriptRoot $launcher.Script
        $shortcut = $shell.CreateShortcut($path)
        $shortcut.TargetPath = $powershell
        $shortcut.Arguments = '-NoProfile -ExecutionPolicy Bypass ' + $launcher.Options + '-File "' + $script + '" -ShowErrors'
        $shortcut.WorkingDirectory = $root
        $shortcut.IconLocation = (Join-Path $root 'assets\icons\app-icon.ico') + ',0'
        $shortcut.Description = $launcher.Description
        $shortcut.Save()
        Write-Host "Created $path"
    }
}
