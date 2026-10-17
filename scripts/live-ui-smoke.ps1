param([Parameter(Mandatory=$true)][string]$ExePath,[string]$LiveUsername)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$exe=(Resolve-Path -LiteralPath $ExePath).Path
$package=Split-Path -Parent $exe
$node=Join-Path $package 'resources\node\node.exe'
if(!(Test-Path -LiteralPath $node)){throw 'Use the complete packaged application with its bundled Node.'}
$scratch=Join-Path $root ('.runtime\live-ui-smoke-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$settings=@{
    CONCAT_LIVE_SMOKE_USERNAME='fixture_creator'
    CONCAT_TIKTOK_LIVE_CONNECTOR=(Join-Path $root 'tools\live-smoke\connector.cjs')
    CONCAT_TIKTOK_LIVE_NODE=$node
    LOCALAPPDATA=(Join-Path $scratch 'local')
    APPDATA=(Join-Path $scratch 'roaming')
    TEMP=$scratch
    TMP=$scratch
    RUST_LOG='info'
}
if($LiveUsername){
    if($LiveUsername-notmatch '^@?[A-Za-z0-9_.]{1,24}$'){throw 'Invalid public TikTok username.'}
    $settings.CONCAT_LIVE_SMOKE_USERNAME=$LiveUsername
    $settings.CONCAT_TIKTOK_LIVE_CONNECTOR=Join-Path $package 'resources\tiktok-live-connector\dist\artcraftLiveRuntime.js'
}
$previous=@{}
try {
    foreach($name in $settings.Keys){
        $previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')
        [Environment]::SetEnvironmentVariable($name,$settings[$name],'Process')
    }
    $stdout=Join-Path $scratch 'stdout.txt'
    $stderr=Join-Path $scratch 'stderr.txt'
    $process=Start-Process -FilePath $exe -ArgumentList '--smoke' -WorkingDirectory $package -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    # Cache the process handle before a short-lived GUI process exits; otherwise
    # Windows PowerShell can report a null ExitCode after WaitForExit().
    $null=$process.Handle
    # The caller may continue receiving progress while the explicit smoke runs.
    if(!$process.WaitForExit(60000)){$process.Kill();throw 'LIVE UI smoke exceeded its bounded deadline.'}
    $process.Refresh()
    if($null-eq $process.ExitCode -or $process.ExitCode-ne 0){throw "LIVE UI smoke failed: exit $($process.ExitCode). Isolated logs: $scratch"}
    if(!(Select-String -LiteralPath $stdout -SimpleMatch 'LIVE_UI_SMOKE=PASS' -Quiet)){
        throw "Missing LIVE UI round-trip marker. This may be an old executable; startup exit 0 alone is not a LIVE test. Logs: $scratch"
    }
    if($LiveUsername){Write-Output 'LIVE_UI_SMOKE=PASS (real public TikTok LIVE -> real App UI -> disconnect; no posting or AI)'}
    else {Write-Output 'LIVE_UI_SMOKE=PASS (real App callbacks, delayed fixture connection, cross-thread UI update and disconnect; no TikTok/AI traffic)'}
    Write-Output "SMOKE_LOGS=$scratch"
} finally {
    foreach($name in $previous.Keys){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
