#requires -Version 5.1
# Build the current checkout before launching; never fall back to an older binary.
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string]$Repository,
    [switch]$BuildOnly,
    [switch]$ShowErrors
)

$ErrorActionPreference = 'Stop'

try {
    $root = Split-Path $PSScriptRoot -Parent
    $target = Join-Path $root 'target'
    $binary = Join-Path $target 'debug\rgitui.exe'
    $start = @{
        FilePath = $binary
        WorkingDirectory = $root
    }
    if ($Repository) {
        # Resolve relative paths before changing directory for Cargo.
        $repo = Get-Item -LiteralPath $Repository
        if (-not $repo.PSIsContainer -or $repo.PSProvider.Name -ne 'FileSystem') {
            throw "Repository must be a directory: $Repository"
        }
        $start.ArgumentList = '"' + ($repo.FullName -replace '(\\+)$', '$1$1') + '"'
    }

    $running = Get-Process rgitui -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $binary }
    if ($running) {
        throw 'Close rgitui, then use this shortcut again. Windows cannot replace the app while it is running. No processes were stopped.'
    }
    $cargo = Get-Command cargo -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $cargo) {
        throw 'Cargo was not found. Install Rust with rustup, then sign out and back in so desktop shortcuts receive the updated PATH.'
    }

    Write-Host 'Building rgitui from the current local source (no git pull)...'
    Push-Location -LiteralPath $root
    try {
        & $cargo.Source build --package rgitui --target-dir $target
        if ($LASTEXITCODE -ne 0) {
            throw "Build failed (exit code $LASTEXITCODE). See the build output above. No app was launched."
        }
    } finally {
        Pop-Location
    }
    if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) {
        throw "Build did not produce $binary. This shortcut requires a native Windows debug build."
    }
    if (-not $BuildOnly) {
        Write-Host 'Starting the freshly built rgitui...'
        Start-Process @start
    }
} catch {
    if ($ShowErrors) {
        Add-Type -AssemblyName System.Windows.Forms
        [void][System.Windows.Forms.MessageBox]::Show(
            $_.Exception.Message, 'rgitui build & run', 'OK', 'Error')
    }
    Write-Error $_
    exit 1
}
