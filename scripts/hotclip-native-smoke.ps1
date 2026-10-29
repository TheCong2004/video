param([string]$ExePath, [switch]$Preview)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (!$ExePath) { $ExePath=Join-Path $repo 'src\target\debug\concat.exe' }
$exe=(Resolve-Path -LiteralPath $ExePath).Path
if (Test-Path -LiteralPath (Join-Path (Split-Path $exe) 'portable')) { throw 'Smoke requires isolated non-portable build.' }
$scratch=Join-Path $repo ('.runtime\hotclip-qa-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$ffmpeg=@(Get-Item -Path (Join-Path $repo '.deps\ffmpeg\*\bin'))
if($ffmpeg.Count -ne 1){throw 'Expected installed FFmpeg runtime'}
$previous=@{}
$settings=@{
    APPDATA=(Join-Path $scratch 'roaming'); LOCALAPPDATA=(Join-Path $scratch 'local'); TEMP=$scratch; TMP=$scratch
    CONCAT_HOTCLIP_SMOKE='1'; CONCAT_HOTCLIP_SMOKE_MEDIA=(Join-Path $repo '.runtime\live-director-av-fixture.mp4')
    CONCAT_NEXPULSE_SMOKE=$null; CONCAT_VOICE_SELECTION_SMOKE=$null; CONCAT_LIVE_SYNCED_SMOKE=$null
    CONCAT_LIVE_SMOKE_USERNAME=$null; CONCAT_LIVE_MOTION_SMOKE=$null; CONCAT_PROMAX_AVATAR_SMOKE_VIDEO=$null
    PATH=(Split-Path $exe)+';'+$ffmpeg[0].FullName+';'+$env:PATH
}
try {
    foreach($key in $settings.Keys){$previous[$key]=[Environment]::GetEnvironmentVariable($key,'Process');[Environment]::SetEnvironmentVariable($key,$settings[$key],'Process')}
    $stdout=Join-Path $scratch 'stdout.txt';$stderr=Join-Path $scratch 'stderr.txt'
    $qaArguments=if($Preview){'--hotclip-smoke'}else{'--smoke'}
    $child=Start-Process -FilePath $exe -ArgumentList $qaArguments -WorkingDirectory $repo -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $null=$child.Handle
    if(!$child.WaitForExit(60000)){$child.Kill();throw "Owned smoke timeout: $scratch"}
    $child.Refresh()
    if($child.ExitCode -ne 0 -or !(Select-String -LiteralPath $stdout -SimpleMatch 'HOTCLIP_NATIVE_SMOKE=PASS' -Quiet)){throw "Hotclip native smoke failed: $scratch"}
    if(!(Select-String -LiteralPath $stdout -SimpleMatch 'HOTCLIP_GEOMETRY_SMOKE=PASS' -Quiet)){throw "Missing actual native geometry recovery proof: $scratch"}
    $pidLine=Select-String -LiteralPath $stdout -Pattern '^HOTCLIP_OWNED_PID=(\d+)$'
    if(!$pidLine){throw 'Missing owned Hotclip PID receipt'}
    $ownedHotclipPid=[int]$pidLine.Matches[0].Groups[1].Value
    if(Get-Process -Id $ownedHotclipPid -ErrorAction SilentlyContinue){throw 'Owned Hotclip remained after host exit'}
    Get-Content -LiteralPath $stdout
    "HOTCLIP_SHUTDOWN=PASS; SMOKE_LOGS=$scratch"
}finally{foreach($key in $previous.Keys){[Environment]::SetEnvironmentVariable($key,$previous[$key],'Process')}}
