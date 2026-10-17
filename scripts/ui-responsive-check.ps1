param([string]$PreviewExecutable, [string]$OutputDirectory)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if(!$PreviewExecutable){$PreviewExecutable=Join-Path $repo 'src\target\debug\concat-studio-preview.exe'}
if(!$OutputDirectory){$OutputDirectory=Join-Path $repo '.runtime\ui-responsive'}
$exe=(Resolve-Path -LiteralPath $PreviewExecutable).Path
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
# Synthetic layout state only: no browser, keys, provider calls or user project.
foreach($route in @('keys','keys-sample','keys-form','ai-edit')){
    foreach($width in @(320,360,560,960,1440)){
        $png=Join-Path $OutputDirectory "$route-$width.png"
        & $exe $png $route $width 900
        if($LASTEXITCODE-ne 0){throw "Native layout failed: $route at ${width}px"}
    }
}
& $exe (Join-Path $OutputDirectory 'ai-edit-input.png') ai-edit 360 900 input-smoke
if($LASTEXITCODE-ne 0){throw 'Native prompt/clipboard event smoke failed.'}
Write-Output 'RESPONSIVE_LAYOUT=PASS (20 native renders + isolated input smoke)'
