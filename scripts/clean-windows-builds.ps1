#requires -Version 5.1
<#
.SYNOPSIS
Removes only known legacy Windows previews under this repository's target folder.
.DESCRIPTION
Dry run by default. Use -Apply to delete, and -Incremental to also include the
Debug/Release incremental caches. Reparse points (including junctions) cause
an entire candidate to be skipped. Stop builds and the app before applying;
do not modify target concurrently. Sizes are logical file bytes, not disk usage.
.EXAMPLE
powershell -NoProfile -File scripts/clean-windows-builds.ps1 -Incremental
.EXAMPLE
powershell -NoProfile -File scripts/clean-windows-builds.ps1 -Apply
#>
[CmdletBinding()]
param(
    [switch]$Apply,
    [switch]$Incremental
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Assert-NoBuildProcesses {
    $busy = @(Get-Process -ErrorAction Stop | Where-Object {
        $_.ProcessName -in @('cargo', 'rustc', 'rgitui')
    })
    if ($busy.Count -gt 0) {
        $names = ($busy | ForEach-Object { '{0} (PID {1})' -f $_.ProcessName, $_.Id }) -join ', '
        throw "Refusing -Apply while these processes are running: $names. Close them and retry; no processes were killed."
    }
}

function Get-OptionalItem([string]$Path) {
    try {
        Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    }
    catch [System.Management.Automation.ItemNotFoundException] {
        return $null
    }
}

function Assert-NoReparsePoint($Item) {
    if (($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Reparse point is not allowed: $($Item.FullName)"
    }
}

# Check parents as well: target, debug/release, or even the repo may be a junction.
function Assert-SafeAncestors([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    $parent = [IO.Path]::GetDirectoryName($current)
    if ($parent) { Assert-SafeAncestors $parent }
    $item = Get-OptionalItem $current
    if ($null -ne $item) {
        Assert-NoReparsePoint $item
    }
}

# Enumerate one level at a time; never use recursive enumeration or deletion.
# A complete read-only pass must succeed before a candidate can be deleted.
function Invoke-SafeTree([string]$Path, [switch]$Delete) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    Assert-NoReparsePoint $item
    [long]$bytes = 0
    if ($item.PSIsContainer) {
        foreach ($child in @(Get-ChildItem -LiteralPath $Path -Force -ErrorAction Stop)) {
            $bytes += Invoke-SafeTree -Path $child.FullName -Delete:$Delete
        }
        if ($Delete) {
            # Nonrecursive deletion fails safely if new children have appeared.
            [IO.Directory]::Delete($Path, $false)
        }
    }
    else {
        $bytes = $item.Length
        if ($Delete) {
            [IO.File]::Delete($Path)
            $script:removedBytes += $bytes
        }
    }
    return $bytes
}

function Format-Bytes([long]$Bytes) {
    return ('{0:N2} MiB ({1:N0} bytes)' -f ($Bytes / 1MB), $Bytes)
}

$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$target = Join-Path $repo 'target'
$script:removedBytes = [long]0
if ($Apply) { Assert-NoBuildProcesses }
Assert-SafeAncestors $target
$targetItem = Get-OptionalItem $target
if ($null -eq $targetItem) {
    Write-Output "Nothing to clean: $target does not exist."
    return
}
if (-not $targetItem.PSIsContainer) { throw "Not a directory: $target" }

# Fixed allowlist only. No wildcard matching, caller-supplied root, or cargo clean.
$candidates = @(
    'try-forest'
    'try-graph'
    'try-graph-polish'
    'try-multirepo'
    'rgitui-forest-preview-windows-x64.zip'
    'rgitui-graph-preview-windows-x64.zip'
    'rgitui-graph-polish-preview-windows-x64.zip'
    'rgitui-multirepo-preview-windows-x64.zip'
)
if ($Incremental) {
    $candidates += 'debug/incremental', 'release/incremental'
}

$plan = @()
[long]$estimatedBytes = 0
$failures = 0
Write-Output "Target: $target"
foreach ($relative in $candidates) {
    $path = Join-Path $target $relative
    try {
        Assert-SafeAncestors $path
        $item = Get-OptionalItem $path
        if ($null -eq $item) {
            Write-Output "Absent: $relative"
            continue
        }
        $expectDirectory = -not $relative.EndsWith('.zip')
        if ($item.PSIsContainer -ne $expectDirectory) {
            throw "Unexpected file/directory type: $path"
        }
        [long]$bytes = Invoke-SafeTree -Path $path
        $estimatedBytes += $bytes
        $plan += [pscustomobject]@{ Path = $path; Relative = $relative }
        Write-Output ("Eligible: {0} - {1}" -f $relative, (Format-Bytes $bytes))
    }
    catch {
        $failures++
        Write-Warning "Skipped $relative : $($_.Exception.Message)"
    }
}
Write-Output ("Estimated eligible file size: {0}" -f (Format-Bytes $estimatedBytes))
if (-not $Apply) {
    Write-Output 'Dry run: nothing deleted. Use -Apply to delete the eligible paths.'
    return
}

try {
    foreach ($entry in $plan) {
        # Repeat after sizing, which can take time. Never terminate a process.
        Assert-NoBuildProcesses
        try {
            Assert-SafeAncestors $entry.Path
            # Recheck the whole tree before deleting if it changed since planning.
            $null = Invoke-SafeTree -Path $entry.Path
            Assert-NoBuildProcesses
            $null = Invoke-SafeTree -Path $entry.Path -Delete
            Write-Output "Removed: $($entry.Relative)"
        }
        catch {
            $failures++
            Write-Warning "Incomplete cleanup of $($entry.Relative): $($_.Exception.Message)"
        }
    }
}
finally {
    Write-Output ("Removed file bytes (estimated reclaimed): {0}" -f (Format-Bytes $script:removedBytes))
    Write-Output 'This is not a measurement of free disk space; hard links, compression, and open files can affect reclamation.'
}
if ($failures -gt 0) {
    throw "$failures path(s) were skipped or could not be fully removed. See warnings; rerunning is safe."
}
