param(
    [Parameter(Mandatory=$true)][string]$Image,
    [Parameter(Mandatory=$true)][string]$Voice,
    [Parameter(Mandatory=$true)][string]$FFmpeg
)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
foreach($file in @($Image,$Voice,$FFmpeg)) {
    if(-not (Test-Path -LiteralPath $file -PathType Leaf)){throw "Missing supplied media or FFmpeg: $file"}
}
$dependency=Join-Path $repo 'src/target/debug/deps'
$controlLib=Get-ChildItem -LiteralPath $dependency -Filter 'libconcat_studio-*.rlib' |
    Sort-Object LastWriteTime -Descending | Select-Object -First 1
if(-not $controlLib){throw 'Build concat-studio with the pinned toolchain first.'}
$binary=Join-Path $repo '.runtime/test-ai-control-render.exe'
New-Item -ItemType Directory -Force -Path (Split-Path $binary) | Out-Null
Push-Location (Join-Path $repo 'src')
try {
    & rustc --edition 2024 --test (Join-Path $PSScriptRoot 'test_ai_control_render.rs') --extern "concat_studio=$($controlLib.FullName)" -L "dependency=$dependency" -o $binary
    if($LASTEXITCODE -ne 0){throw 'Integration test compilation failed.'}
    $env:KEDI_TEST_REPO=$repo
    $env:CONCAT_CONTROL_TEST_IMAGE=(Resolve-Path -LiteralPath $Image).Path
    $env:CONCAT_CONTROL_TEST_VOICE=(Resolve-Path -LiteralPath $Voice).Path
    $env:CONCAT_TEST_FFMPEG=(Resolve-Path -LiteralPath $FFmpeg).Path
    & $binary --nocapture
    if($LASTEXITCODE -ne 0){throw 'Integration test failed; inspect its owned run folder.'}
} finally {Pop-Location}
