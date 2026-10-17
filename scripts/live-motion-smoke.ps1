param([Parameter(Mandatory=$true)][string]$ExePath,[string]$VideoPath)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$exe=(Resolve-Path -LiteralPath $ExePath).Path
if(!$VideoPath){$VideoPath=Join-Path $root 'src\crates\concat\assets\avatars\newscaster.mp4'}
$video=(Resolve-Path -LiteralPath $VideoPath).Path
$scratch=Join-Path $root ('.runtime\live-motion-smoke-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$ffmpeg=Get-ChildItem -Path (Join-Path $root '.deps\ffmpeg\*\bin') -Directory | Select-Object -First 1
$ort=Get-ChildItem -Path (Join-Path $root '.deps\ort\*\lib') -Directory | Select-Object -First 1
$sherpa=Get-ChildItem -Path (Join-Path $root '.deps\sherpa-onnx-*-win-x64-shared-*-lib\lib') -Directory | Select-Object -First 1
$settings=@{
    CONCAT_LIVE_MOTION_SMOKE=$video
    LOCALAPPDATA=(Join-Path $scratch 'local')
    APPDATA=(Join-Path $scratch 'roaming')
    TEMP=$scratch; TMP=$scratch; RUST_LOG='info'
    PATH=(@($ffmpeg.FullName,$ort.FullName,$sherpa.FullName,(Join-Path $root '.deps\sherpa-dlls'),$env:PATH)-join ';')
}
$previous=@{}
try {
    foreach($name in $settings.Keys){
        $previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')
        [Environment]::SetEnvironmentVariable($name,$settings[$name],'Process')
    }
    $stdout=Join-Path $scratch 'stdout.txt'
    $stderr=Join-Path $scratch 'stderr.txt'
    $process=Start-Process -FilePath $exe -ArgumentList '--smoke' -WorkingDirectory (Split-Path -Parent $exe) -WindowStyle Hidden -RedirectStandardOutput $stdout -RedirectStandardError $stderr -PassThru
    $null=$process.Handle
    if(!$process.WaitForExit(55000)){$process.Kill();throw "Motion smoke timed out. Logs: $scratch"}
    $process.Refresh()
    if($null-eq $process.ExitCode -or $process.ExitCode-ne 0){throw "Motion smoke failed: exit $($process.ExitCode). Logs: $scratch"}
    if(!(Select-String -LiteralPath $stdout -SimpleMatch 'LIVE_MOTION_SMOKE=PASS' -Quiet)){
        throw "Missing motion proof (old executable is not a pass). Logs: $scratch"
    }
    Get-Content -LiteralPath $stdout | Select-String 'LIVE_MOTION_SMOKE=PASS'
    Write-Output "SMOKE_LOGS=$scratch"
} finally {
    foreach($name in $previous.Keys){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
