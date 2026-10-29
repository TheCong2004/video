param([string]$PreviewExecutable, [string]$OutputDirectory)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if(!$PreviewExecutable){$PreviewExecutable=Join-Path $repo 'src\target\debug\concat-studio-preview.exe'}
if(!$OutputDirectory){$OutputDirectory=Join-Path $repo '.runtime\ui-responsive'}
$exe=(Resolve-Path -LiteralPath $PreviewExecutable).Path
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
# Synthetic layout state only: no browser, keys, provider calls or user project.
foreach($route in @('keys','keys-sample','keys-form','ai-edit','ai-edit-api','ai-edit-manual','live','live-connected','live-error','live-loading','live-partial','live-voice','production','bulk','channels','publish','settings','scripts','auto-shorts','mc','promax-avatars','promax-offline')){
    foreach($width in @(320,360,560,960,1440)){
        $png=Join-Path $OutputDirectory "$route-$width.png"
        & $exe $png $route $width 900
        if($LASTEXITCODE-ne 0){throw "Native layout failed: $route at ${width}px"}
    }
}
& $exe (Join-Path $OutputDirectory 'ai-edit-input.png') ai-edit 360 900 input-smoke
if($LASTEXITCODE-ne 0){throw 'Native prompt/clipboard event smoke failed.'}
& $exe (Join-Path $OutputDirectory 'clipboard-input.png') ai-edit-manual 1080 900 clipboard-smoke
if($LASTEXITCODE-ne 0){throw 'Native clipboard smoke failed.'}
& $exe (Join-Path $OutputDirectory 'live-input.png') live 1600 900 live-input
if($LASTEXITCODE-ne 0){throw 'Native LIVE keyboard/pointer smoke failed.'}
& $exe (Join-Path $OutputDirectory 'avatar-input.png') avatar-picker-input 360 900
if($LASTEXITCODE-ne 0){throw 'Native avatar picker smoke failed.'}
& $exe (Join-Path $OutputDirectory 'live-scroll.png') live-connected 360 900 scroll-bottom
if($LASTEXITCODE-ne 0){throw 'Narrow LIVE page cannot reach lower controls by scrolling.'}
& $exe (Join-Path $OutputDirectory 'launcher-production.png') launcher-production 1080 720
if($LASTEXITCODE-ne 0){throw 'Launcher sidebar -> production navigation failed.'}
& $exe (Join-Path $OutputDirectory 'launcher-resize.png') launcher-resize 1080 720
if($LASTEXITCODE-ne 0){throw 'LIVE resize/minimize/restore regression failed.'}
Write-Output 'RESPONSIVE_LAYOUT=PASS (110 native renders + 7 isolated native input/resize checks)'
