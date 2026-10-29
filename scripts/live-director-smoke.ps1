param([string]$ExePath, [Parameter(Mandatory=$true)][string]$FixturePath)
# Offline native device-clock test. No TikTok, TTS or GPU request is made.
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (!$ExePath) { $ExePath = Join-Path $repo 'src\target\debug\concat.exe' }
$built = (Resolve-Path -LiteralPath $ExePath).Path
$fixture = (Resolve-Path -LiteralPath $FixturePath).Path
$scratch = Join-Path $repo ('.runtime\director-smoke-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
# Run the built binary directly: no accumulating EXE copies or hard links.
# Portable mode ignores APPDATA overrides, so refuse it rather than touching
# the user's settings. A normal dev build uses the isolated paths below.
if (Test-Path -LiteralPath (Join-Path (Split-Path -Parent $built) 'portable')) {
    throw 'Offline isolation requires a non-portable dev executable.'
}
$ffmpegBin = @(Get-Item -Path (Join-Path $repo '.deps\ffmpeg\*\bin') | Where-Object { $_.PSIsContainer })
if ($ffmpegBin.Count -ne 1) { throw 'Expected the installed FFmpeg runtime.' }
$settings = @{
    APPDATA = (Join-Path $scratch 'roaming')
    LOCALAPPDATA = (Join-Path $scratch 'local')
    TEMP = $scratch
    TMP = $scratch
    CONCAT_LIVE_SYNCED_SMOKE = $fixture
    CONCAT_VOICE_SELECTION_SMOKE = '1'
    CONCAT_LIVE_SMOKE_USERNAME = $null
    CONCAT_PROMAX_AVATAR_SMOKE_VIDEO = $null
    CONCAT_LIVE_MOTION_SMOKE = $null
    PATH = (Split-Path -Parent $built) + ';' + $ffmpegBin[0].FullName + ';' + $env:PATH
}
$previous = @{}
try {
    foreach ($name in $settings.Keys) {
        $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
        [Environment]::SetEnvironmentVariable($name, $settings[$name], 'Process')
    }
    $stdout = Join-Path $scratch 'stdout.txt'
    $stderr = Join-Path $scratch 'stderr.txt'
    $child = Start-Process -FilePath $built -ArgumentList '--smoke' -WorkingDirectory $repo -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $null = $child.Handle
    if (!$child.WaitForExit(55000)) { $child.Kill(); throw "Owned smoke timed out; logs: $scratch" }
    $child.Refresh()
    if ($child.ExitCode -ne 0 -or !(Select-String -LiteralPath $stdout -SimpleMatch 'LIVE_SYNCED_SMOKE=PASS' -Quiet) -or !(Select-String -LiteralPath $stdout -SimpleMatch 'VOICE_SELECTION_SMOKE=PASS' -Quiet)) {
        throw "Native A/V test failed, exit $($child.ExitCode); logs: $scratch"
    }
    Get-Content -LiteralPath $stdout
    "SMOKE_LOGS=$scratch"
} finally {
    foreach ($name in $previous.Keys) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
