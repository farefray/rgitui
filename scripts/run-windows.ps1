# Launch the newest completed local build, not a dated preview copy.
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Repository,
    [switch]$PrintPath,
    [switch]$ShowErrors
)

$ErrorActionPreference = 'Stop'

try {
    $root = Split-Path $PSScriptRoot -Parent
    $binary = @('target\release\rgitui.exe', 'target\debug\rgitui.exe') |
        ForEach-Object {
            $path = Join-Path $root $_
            if (Test-Path -LiteralPath $path -PathType Leaf) {
                Get-Item -LiteralPath $path
            }
        } |
        Sort-Object LastWriteTimeUtc -Descending |
        Select-Object -First 1

    if (-not $binary) {
        throw "No local rgitui build found. Run 'cargo build --package rgitui' in $root first."
    }
    if ($PrintPath) {
        $binary.FullName
        return
    }

    $start = @{
        FilePath = $binary.FullName
        WorkingDirectory = (Get-Location).Path
    }
    if ($Repository) {
        $repo = Get-Item -LiteralPath $Repository
        if (-not $repo.PSIsContainer -or $repo.PSProvider.Name -ne 'FileSystem') {
            throw "Repository must be a directory: $Repository"
        }
        # Start-Process joins ArgumentList; quote spaces and double trailing
        # backslashes so Windows argument parsing preserves drive roots too.
        $start.ArgumentList = '"' + ($repo.FullName -replace '(\\+)$', '$1$1') + '"'
    }
    Start-Process @start
} catch {
    if ($ShowErrors) {
        Add-Type -AssemblyName System.Windows.Forms
        [void][System.Windows.Forms.MessageBox]::Show(
            $_.Exception.Message, 'rgitui launcher', 'OK', 'Error')
    }
    Write-Error $_
    exit 1
}
