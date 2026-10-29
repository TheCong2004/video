param([string]$ExePath, [switch]$NativeApp)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (!$ExePath) { $ExePath = Join-Path $repo '.runtime\nexpulse-build\debug\concat-webview-preview.exe' }
$exe = (Resolve-Path -LiteralPath $ExePath).Path
$scratch = Join-Path $repo ('.runtime\nexpulse-qa-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$previous = @{}
try {
    $ffmpegBin = @(Get-Item -Path (Join-Path $repo '.deps\ffmpeg\*\bin') | Where-Object { $_.PSIsContainer })
    if ($NativeApp -and $ffmpegBin.Count -ne 1) { throw 'Expected installed FFmpeg runtime' }
    foreach ($entry in @{
        LOCALAPPDATA = (Join-Path $scratch 'local')
        APPDATA = (Join-Path $scratch 'roaming')
        TEMP = $scratch
        TMP = $scratch
        SLINT_BACKEND = $(if ($NativeApp) { $env:SLINT_BACKEND } else { 'winit-software' })
        CONCAT_NEXPULSE_SMOKE = $(if ($NativeApp) { '1' } else { $null })
        CONCAT_LIVE_SYNCED_SMOKE = $null
        CONCAT_LIVE_SMOKE_USERNAME = $null
        CONCAT_PROMAX_AVATAR_SMOKE_VIDEO = $null
        CONCAT_LIVE_MOTION_SMOKE = $null
        PATH = $(if ($NativeApp) { (Split-Path -Parent $exe) + ';' + $ffmpegBin[0].FullName + ';' + $env:PATH } else { $env:PATH })
    }.GetEnumerator()) {
        $previous[$entry.Key] = [Environment]::GetEnvironmentVariable($entry.Key, 'Process')
        [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value, 'Process')
    }
    $stdout = Join-Path $scratch 'stdout.txt'
    $stderr = Join-Path $scratch 'stderr.txt'
    $child = Start-Process -FilePath $exe -ArgumentList '--smoke' -WorkingDirectory $repo -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $null = $child.Handle
    if (!$child.WaitForExit(60000)) {
        # Only this explicitly created test process, never Concat or a user browser.
        $child.Kill()
        throw "Owned NexPulse smoke timed out; logs: $scratch"
    }
    $child.Refresh()
    if ($child.ExitCode -ne 0 -or !(Select-String -LiteralPath $stdout -SimpleMatch 'NEXPULSE_WEB_SMOKE=PASS' -Quiet)) {
        throw "NexPulse smoke failed, exit $($child.ExitCode); logs: $scratch"
    }
    Get-Content -LiteralPath $stdout
    "SMOKE_LOGS=$scratch"
} finally {
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
