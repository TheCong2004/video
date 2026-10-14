param([switch]$Apply)

# Only regenerable Concat build artifacts. Source, .deps, models, user projects
# and third-party Cargo libraries are never candidates. Default is inspection.
$ErrorActionPreference = 'Stop'
$cleanupRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$targetRoot = Join-Path $cleanupRoot 'src\target'
$tempRoot = Join-Path $cleanupRoot '.runtime\native-temp'
$cutoff = [DateTime]::UtcNow.AddHours(-24)
$scratchCutoff = [DateTime]::UtcNow.AddHours(-1)

function Assert-CleanupPath([string]$Path) {
    $resolved = [IO.Path]::GetFullPath($Path)
    $allowed = $resolved.StartsWith($targetRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
        $resolved.StartsWith($tempRoot + '\', [StringComparison]::OrdinalIgnoreCase)
    if (!$allowed) { throw "Refusing cleanup outside generated build/temp folders: $resolved" }
    # Reject junctions/symlinks in the full ancestry, not only the leaf.
    $ancestor = $resolved
    while ($ancestor -and $ancestor.Length -ge $cleanupRoot.Length) {
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
    $_.ProcessName -match '^concat_(agent|project|host|studio)-[0-9a-f]+$'
})
if ($busy.Count) { Write-Host '[cleanup] skipped: a compiler/test process is active'; return }

$candidates = [Collections.Generic.List[string]]::new()
foreach ($profile in @('debug','quick')) {
    $profileRoot = Join-Path $targetRoot $profile
    $incremental = Join-Path $profileRoot 'incremental'
    if (Test-Path -LiteralPath $incremental) {
        # Keep the two newest Concat configurations plus every recent one.
        # Third-party incremental caches are left alone.
        $dirs = @(Get-ChildItem -LiteralPath $incremental -Directory -Force |
            Where-Object { $_.Name -match '^concat-[a-z0-9]+$' } |
            Sort-Object LastWriteTimeUtc -Descending)
        for ($i=2; $i -lt $dirs.Count; $i++) {
            if ($dirs[$i].LastWriteTimeUtc -lt $cutoff) { $candidates.Add($dirs[$i].FullName) }
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
            $file.LastWriteTimeUtc -lt $scratchCutoff
        # Test executables/symbols are disposable after the result is known.
        # Keep .rlib/.rmeta dependencies and the runnable GUI concat.exe.
        $testOutput = $file.Name -match '^concat_(agent|project|host|studio)-[0-9a-f]{16}\.(exe|pdb)$'
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
