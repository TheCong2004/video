param(
    [switch]$Apply,
    [switch]$PruneOwnCache,
    [switch]$PruneMainIncremental,
    [ValidateRange(1, 2)]
    [int]$KeepMainConfigurations = 2,
    [string]$OwnedTempRoot,
    [string]$OwnedPackageFile
)

# Only regenerable Concat build artifacts. Source, .deps, models, user projects
# and third-party Cargo libraries are never candidates. Default is inspection.
$ErrorActionPreference = 'Stop'
$cleanupRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$workspaceRoot = [IO.Path]::GetFullPath((Split-Path -Parent $cleanupRoot))
$targetRoot = Join-Path $cleanupRoot 'src\target'
$tempRoot = Join-Path $cleanupRoot '.runtime\native-temp'
$ownedTempResolved = $null
$ownedPackageResolved = $null
if ($OwnedTempRoot) {
    $ownedTempResolved = [IO.Path]::GetFullPath($OwnedTempRoot)
    $ownedName = [IO.Path]::GetFileName($ownedTempResolved)
    if ([IO.Path]::GetDirectoryName($ownedTempResolved) -ne $workspaceRoot -or
        $ownedName -notmatch '^\.codex-ai-shorts-temp-[a-f0-9]{32}$') {
        throw "Refusing unrecognized task temp root: $ownedTempResolved"
    }
}
if ($OwnedPackageFile) {
    $ownedPackageResolved = [IO.Path]::GetFullPath($OwnedPackageFile)
    $expectedPackageDir = Join-Path $cleanupRoot 'dist\windows'
    $packageName = [IO.Path]::GetFileName($ownedPackageResolved)
    if ([IO.Path]::GetDirectoryName($ownedPackageResolved) -ne $expectedPackageDir -or
        $packageName -notmatch '^ConcatStudio-AutoShorts-\d{8}-\d{6}\.zip$') {
        throw "Refusing unrecognized task package file: $ownedPackageResolved"
    }
}
$cutoff = [DateTime]::UtcNow.AddHours(-24)
$scratchCutoff = [DateTime]::UtcNow.AddHours(-1)
# Low-disk builds must not retain every recent preview/failed configuration.
# This changes only regenerable caches owned by Concat, never dependencies.
$driveRoot = [IO.Path]::GetPathRoot($cleanupRoot)
$lowDisk = ([IO.DriveInfo]::new($driveRoot)).AvailableFreeSpace -lt 8GB
$pruneOwn = $PruneOwnCache.IsPresent -or $lowDisk

function Assert-CleanupPath([string]$Path) {
    $resolved = [IO.Path]::GetFullPath($Path)
    $allowed = $resolved.StartsWith($targetRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
        $resolved.StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
        ($ownedTempResolved -and $resolved -eq $ownedTempResolved) -or
        ($ownedPackageResolved -and $resolved -eq $ownedPackageResolved)
    if (!$allowed) { throw "Refusing cleanup outside generated build/temp folders: $resolved" }
    # Reject junctions/symlinks in the full ancestry, not only the leaf.
    $ancestor = $resolved
    $ancestorLimit = if ($ownedTempResolved -and $resolved -eq $ownedTempResolved) { $workspaceRoot } else { $cleanupRoot }
    while ($ancestor -and $ancestor.Length -ge $ancestorLimit.Length) {
        if (Test-Path -LiteralPath $ancestor) {
            $item = Get-Item -LiteralPath $ancestor -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Refusing cleanup through reparse point: $ancestor"
            }
        }
        $ancestor = Split-Path -Parent $ancestor
    }
    return $resolved
}

# Never race a compiler or native test process, including another terminal.
$busy = @(Get-Process -ErrorAction SilentlyContinue | Where-Object {
    $_.ProcessName -in @('cargo','rustc','link','cl','cmake','ninja') -or
    $_.ProcessName -match '^(concat_(agent|project|host|studio)|live_speech)-[0-9a-f]+$'
})
if ($busy.Count) { Write-Host '[cleanup] skipped: a compiler/test process is active'; return }

$candidates = [Collections.Generic.List[string]]::new()
if (!$ownedTempResolved -and !$ownedPackageResolved) {
foreach ($profile in @('debug','quick')) {
    $profileRoot = Join-Path $targetRoot $profile
    $incremental = Join-Path $profileRoot 'incremental'
    if (Test-Path -LiteralPath $incremental) {
        # Keep the requested number of newest Concat configurations plus recent ones.
        # Third-party incremental caches are left alone.
        $dirs = @(Get-ChildItem -LiteralPath $incremental -Directory -Force |
            Where-Object { $_.Name -match '^concat-[a-z0-9]+$' } |
            Sort-Object LastWriteTimeUtc -Descending)
        for ($i=$KeepMainConfigurations; $i -lt $dirs.Count; $i++) {
            if ($pruneOwn -or $dirs[$i].LastWriteTimeUtc -lt $cutoff) { $candidates.Add($dirs[$i].FullName) }
        }
        if ($PruneMainIncremental) {
            # Explicitly discard only this application's regenerable incremental
            # state, including interrupted *-working sessions. Keep EXE, linked
            # libraries and every dependency. Busy-process and Cargo-lock guards
            # above/below still apply.
            foreach ($dir in $dirs) {
                if (!$candidates.Contains($dir.FullName)) { $candidates.Add($dir.FullName) }
            }
        }
        if ($pruneOwn) {
            # Preview caches are test-only; quick-profile editor caches cannot
            # produce the supported Windows native executable. Keep debug's two
            # latest main configurations to avoid rebuilding all dependencies.
            foreach ($dir in @(Get-ChildItem -LiteralPath $incremental -Directory -Force)) {
                if ($dir.Name -match '^concat_studio_preview-[a-z0-9]+$' -or
                    ($profile -eq 'quick' -and $dir.Name -match '^concat-[a-z0-9]+$')) {
                    if (!$candidates.Contains($dir.FullName)) { $candidates.Add($dir.FullName) }
                }
            }
        }
        # Cargo keeps previous session snapshots inside the same configuration.
        # Keep its newest completed session. Previous completed snapshots are
        # redundant once compilation has stopped; they can each occupy GBs.
        foreach ($dir in $dirs) {
            if ($candidates.Contains($dir.FullName)) { continue }
            $sessions = @(Get-ChildItem -LiteralPath $dir.FullName -Directory -Force |
                Where-Object { $_.Name -match '^s-[a-z0-9-]+$' -and $_.Name -notlike '*-working' } |
                Sort-Object LastWriteTimeUtc -Descending)
            for ($i=1; $i -lt $sessions.Count; $i++) {
                $candidates.Add($sessions[$i].FullName)
            }
        }
    }
    $deps = Join-Path $profileRoot 'deps'
    if (!(Test-Path -LiteralPath $deps)) { continue }
    $files = @(Get-ChildItem -LiteralPath $deps -File -Force)
    $libraries = @($files | Where-Object { $_.Name -match '^libconcat-[0-9a-f]{16}\.rlib$' } |
        Sort-Object LastWriteTimeUtc -Descending)
    $metadata = @($files | Where-Object { $_.Name -match '^libconcat-[0-9a-f]{16}\.rmeta$' } |
        Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 2)
    $keep = @($metadata | ForEach-Object { [IO.Path]::GetFileNameWithoutExtension($_.Name) })
    if ($libraries.Count) { $keep += [IO.Path]::GetFileNameWithoutExtension($libraries[0].Name) }
    foreach ($file in $files) {
        $oldOwnLibrary = $file.Name -match '^libconcat-[0-9a-f]{16}\.(rlib|rmeta)$' -and
            [IO.Path]::GetFileNameWithoutExtension($file.Name) -notin $keep -and $file.LastWriteTimeUtc -lt $cutoff
        # Standalone .rcgu.o files are abandoned compiler intermediates; the
        # reusable linked library is .rlib. Do not remove objects in build/.
        $abandonedObject = $file.Name -match '^concat-[0-9a-f]{16}\..+\.rcgu\.o$' -and
            ($pruneOwn -or $file.LastWriteTimeUtc -lt $scratchCutoff)
        # Test executables/symbols are disposable after the result is known.
        # Keep .rlib/.rmeta dependencies and the runnable GUI concat.exe.
        $testOutput = $file.Name -match '^(concat_(agent|project|host|studio)|live_speech)-[0-9a-f]{16}\.(exe|pdb)$'
        if ($oldOwnLibrary -or $abandonedObject -or $testOutput) { $candidates.Add($file.FullName) }
    }
    foreach ($dir in @(Get-ChildItem -LiteralPath $deps -Directory -Force)) {
        if ($dir.Name -match '^rmeta[a-zA-Z0-9]+$' -and $dir.LastWriteTimeUtc -lt $scratchCutoff) {
            $candidates.Add($dir.FullName)
        }
    }
}
if (Test-Path -LiteralPath $tempRoot) {
    foreach ($dir in @(Get-ChildItem -LiteralPath $tempRoot -Directory -Force)) {
        if ($dir.Name -match '^concat-draft-render-[0-9]+$' -or
            ($dir.Name -match '^rustc[a-zA-Z0-9]+$' -and $dir.LastWriteTimeUtc -lt $scratchCutoff)) {
            $candidates.Add($dir.FullName)
        }
    }
}
}
if ($ownedTempResolved -and (Test-Path -LiteralPath $ownedTempResolved)) {
    $candidates.Add($ownedTempResolved)
}
if ($ownedPackageResolved -and (Test-Path -LiteralPath $ownedPackageResolved -PathType Leaf)) {
    $candidates.Add($ownedPackageResolved)
}

[long]$removed = 0
$cleanupLocks = [Collections.Generic.List[IO.FileStream]]::new()
try {
if ($Apply) {
    # Hold existing Cargo lock files exclusively to close the gap between the
    # process check and deletion. If Cargo owns a lock, do not remove anything.
    foreach ($profile in @('debug','quick')) {
        foreach ($name in @('.cargo-lock','.cargo-build-lock','.cargo-artifact-lock')) {
            $lockPath=Join-Path (Join-Path $targetRoot $profile) $name
            if (Test-Path -LiteralPath $lockPath) {
                $verified=Assert-CleanupPath $lockPath
                try { $cleanupLocks.Add([IO.File]::Open($verified,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)) }
                catch { Write-Host '[cleanup] skipped: Cargo lock is in use'; return }
            }
        }
    }
}
foreach ($candidate in $candidates) {
    $resolved = Assert-CleanupPath $candidate
    if (!(Test-Path -LiteralPath $resolved)) { continue }
    $item = Get-Item -LiteralPath $resolved -Force
    if ($item.PSIsContainer) {
        $children = @(Get-ChildItem -LiteralPath $resolved -Recurse -Force)
        if (@($children | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }).Count) {
            throw "Refusing cleanup of directory containing a reparse point: $resolved"
        }
        $bytes = ($children | Where-Object { !$_.PSIsContainer } | Measure-Object Length -Sum).Sum
    } else { $bytes = $item.Length }
    Write-Host ('[cleanup] {0} {1:N2} GB: {2}' -f $(if ($Apply) {'remove'} else {'candidate'}), ($bytes/1GB), $resolved)
    if ($Apply) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
        $removed += $bytes
    }
}
if ($Apply) { Write-Host ('[cleanup] removed {0:N2} GB of regenerable artifacts' -f ($removed/1GB)) }
} finally {
    foreach ($lock in $cleanupLocks) { $lock.Dispose() }
}
