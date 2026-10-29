param([string]$ExePath)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if(!$ExePath){$ExePath=Join-Path $repo '.runtime\nexpulse-build\debug\concat.exe'}
$exe=(Resolve-Path -LiteralPath $ExePath).Path
if(Test-Path -LiteralPath (Join-Path (Split-Path $exe) 'portable')){throw 'Visible QA requires a non-portable isolated build.'}
$scratch=Join-Path $repo '.runtime\hotclip-visible-qa-20261021'
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
$ffmpeg=@(Get-Item -Path (Join-Path $repo '.deps\ffmpeg\*\bin'))
if($ffmpeg.Count -ne 1){throw 'Expected one FFmpeg runtime'}
$saved=@{}
$settings=@{APPDATA=(Join-Path $scratch 'roaming');LOCALAPPDATA=(Join-Path $scratch 'local');TEMP=$scratch;TMP=$scratch;CONCAT_HOTCLIP_ROOT=(Join-Path (Split-Path $repo) 'hotclip');PATH=(Split-Path $exe)+';'+$ffmpeg[0].FullName+';'+$env:PATH}
try{
foreach($key in $settings.Keys){$saved[$key]=[Environment]::GetEnvironmentVariable($key,'Process');[Environment]::SetEnvironmentVariable($key,$settings[$key],'Process')}
$child=Start-Process -FilePath $exe -WorkingDirectory $repo -WindowStyle Hidden -RedirectStandardOutput (Join-Path $scratch 'final-out.txt') -RedirectStandardError (Join-Path $scratch 'final-err.txt') -PassThru
"VISIBLE_QA_PID=$($child.Id); PROFILE=$scratch"
}finally{foreach($key in $saved.Keys){[Environment]::SetEnvironmentVariable($key,$saved[$key],'Process')}}
