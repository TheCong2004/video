param([switch]$SkipBuild, [switch]$Release)
$ErrorActionPreference='Stop'
$packageRoot=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (!$SkipBuild) {
    if ($Release) { & (Join-Path $PSScriptRoot 'windows-studio.ps1') -Release }
    else { & (Join-Path $PSScriptRoot 'windows-studio.ps1') -Build }
    if ($LASTEXITCODE -ne 0) { throw 'Native build failed; no package was published.' }
}
$profile=if($Release){'release'}else{'debug'}
$built=Join-Path $packageRoot "src\target\$profile\concat.exe"
if (!(Test-Path -LiteralPath $built -PathType Leaf)) { throw 'Missing native executable.' }
$ffmpeg=@(Get-ChildItem -Path (Join-Path $packageRoot '.deps\ffmpeg\*\bin') -Directory)
$ort=@(Get-ChildItem -Path (Join-Path $packageRoot '.deps\ort\*\lib') -Directory)
if($ffmpeg.Count-ne 1 -or $ort.Count-ne 1){throw 'Native runtime directories are ambiguous.'}
$sherpa=@((Join-Path $packageRoot '.deps\sherpa-dlls'))
$sherpa+=@(Get-ChildItem -Path (Join-Path $packageRoot '.deps\sherpa-onnx-*-win-x64-shared-*-lib\lib') -Directory | ForEach-Object {$_.FullName})
$sherpa=$sherpa | Where-Object {Test-Path -LiteralPath (Join-Path $_ 'sherpa-onnx-c-api.dll')} | Select-Object -First 1
if(!$sherpa){throw 'Missing Sherpa runtime DLLs.'}
$output=Join-Path $packageRoot ('dist\windows\Concat-'+(Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
$localAi=Join-Path $packageRoot '.deps\omniroute'
if(!(Test-Path -LiteralPath (Join-Path $localAi '.build\next\BUILD_ID'))){throw 'Stage OmniRoute first: scripts\stage-local-ai.ps1'}
& (Join-Path $localAi 'node.exe') (Join-Path $PSScriptRoot 'harden-local-ai.mjs') $localAi --check
if($LASTEXITCODE -ne 0){throw 'Refusing to package an unverified OmniRoute auth runtime.'}
$live=Join-Path $packageRoot '.deps\tiktok-live-connector'
$mcp=Join-Path $packageRoot 'tools\ai-edit-mcp'
if(!(Test-Path -LiteralPath (Join-Path $mcp 'node_modules\@modelcontextprotocol\sdk\package.json'))){throw 'Install MCP dependencies first: cd tools\ai-edit-mcp; pnpm install --frozen-lockfile --ignore-scripts'}
if(!(Test-Path -LiteralPath (Join-Path $live 'dist\artcraftLiveRuntime.js'))){throw 'Stage TikTok runtime first: scripts\stage-live-runtime.ps1'}
& node (Join-Path $PSScriptRoot 'patch-live-controller.mjs') $live
if($LASTEXITCODE -ne 0){throw 'Concat LIVE controller staging failed.'}
New-Item -ItemType Directory -Path $output -ErrorAction Stop | Out-Null
Copy-Item -LiteralPath $built -Destination (Join-Path $output 'Concat.exe')
Copy-Item -LiteralPath (Join-Path $packageRoot 'docs\WINDOWS-PACKAGE.md') -Destination $output
Copy-Item -LiteralPath (Join-Path $packageRoot 'docs\AI_EDIT_EXTERNAL.md') -Destination $output
Copy-Item -LiteralPath (Join-Path $packageRoot 'docs\TIKTOK_LIVE_CONNECTION_CHECK.md') -Destination $output
Copy-Item -LiteralPath (Join-Path $packageRoot 'docs\LIVE_MOTION.md') -Destination $output
Copy-Item -LiteralPath (Join-Path $packageRoot 'docs\LOCAL_AVATAR_UI.md') -Destination $output
& (Join-Path $PSScriptRoot 'stage-local-avatars.ps1') -Destination (Join-Path $output 'AvatarRuntime')
if(!$?){throw 'Bundling Concat local avatar runtime failed.'}
foreach($tool in @('ffmpeg.exe','ffprobe.exe')) {
    Copy-Item -LiteralPath (Join-Path $ffmpeg[0].FullName $tool) -Destination $output
}
& robocopy $localAi (Join-Path $output 'OmniRoute') /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XF '.env' '.env.*' '*.db' '*.db-*' '*.sqlite' '*.sqlite-*' '*.sqlite3' '*.sqlite3-*' '*.log' | Out-Null
if($LASTEXITCODE -ge 8){throw 'Bundling local AI runtime failed.'}
$liveResources=Join-Path $output 'resources\tiktok-live-connector'
& robocopy $live $liveResources /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XF '.env' '.env.*' '*.db' '*.db-*' '*.log' | Out-Null
if($LASTEXITCODE -ge 8){throw 'Bundling TikTok runtime failed.'}
New-Item -ItemType Directory -Path (Join-Path $output 'resources\node') -Force | Out-Null
Copy-Item -LiteralPath (Join-Path $localAi 'node.exe') -Destination (Join-Path $output 'resources\node\node.exe')
& (Join-Path $PSScriptRoot 'stage-temple-runtime.ps1') -Destination (Join-Path $output 'resources')
New-Item -ItemType Directory -Path (Join-Path $output 'resources\bin') -Force | Out-Null
foreach ($downloader in @('lux.exe', 'yt-dlp.exe')) {
    $downloaderPath = Join-Path $packageRoot "resources\bin\$downloader"
    if (!(Test-Path -LiteralPath $downloaderPath -PathType Leaf)) { throw "Missing online downloader: $downloader" }
    Copy-Item -LiteralPath $downloaderPath -Destination (Join-Path $output 'resources\bin')
}
foreach ($notice in @(Get-ChildItem -LiteralPath (Join-Path $packageRoot 'resources\bin') -Filter 'yt-dlp-*.txt' -File)) {
    Copy-Item -LiteralPath $notice.FullName -Destination (Join-Path $output 'resources\bin')
}
& robocopy $mcp (Join-Path $output 'tools\ai-edit-mcp') /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XD (Join-Path $mcp 'node_modules\.pnpm') /XF '.env' '.env.*' '*.dpapi' '*.log' '*.test.mjs' 'smoke-client.mjs' 'browser-native-client.mjs' 'live-chat-smoke.mjs' 'diagnose-chat.mjs' 'visible-window-smoke.mjs' | Out-Null
if($LASTEXITCODE -ge 8){throw 'Bundling AI Edit MCP runtime failed.'}
foreach($dir in @($ffmpeg[0].FullName,$ort[0].FullName,$sherpa)) {
    foreach($dll in Get-ChildItem -LiteralPath $dir -File -Filter '*.dll') {
        Copy-Item -LiteralPath $dll.FullName -Destination $output -Force
    }
}
$directML=Join-Path $packageRoot "src\target\$profile\DirectML.dll"
if(Test-Path -LiteralPath $directML){Copy-Item -LiteralPath $directML -Destination $output}
# Package the redistributable C runtime when the installed MSVC exposes it.
$crt=@(Get-ChildItem -Path 'C:\Program Files\Microsoft Visual Studio\2022\*\VC\Redist\MSVC\*\x64\Microsoft.VC143.CRT' -Directory -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1)
if($crt.Count){ foreach($dll in Get-ChildItem -LiteralPath $crt[0].FullName -File -Filter '*.dll'){Copy-Item -LiteralPath $dll.FullName -Destination $output -Force} }
foreach($notice in @(Get-ChildItem -LiteralPath $packageRoot -File | Where-Object {$_.Name -match '^(LICENSE|COPYING|NOTICE)'})) {
    Copy-Item -LiteralPath $notice.FullName -Destination $output
}
$thirdParty=Join-Path $packageRoot 'THIRD_PARTY_NOTICES.md'
if(Test-Path -LiteralPath $thirdParty){Copy-Item -LiteralPath $thirdParty -Destination $output}
foreach($required in @('avcodec-62.dll','avformat-62.dll','avutil-60.dll','sherpa-onnx-c-api.dll','onnxruntime.dll')) {
    if(!(Test-Path -LiteralPath (Join-Path $output $required))){throw "Missing packaged dependency: $required"}
}
# DLL loader smoke with no development PATH entries. This does not claim cloud
# login, provider management, optional LIVE/avatars or GPU inference readiness.
$oldPath=$env:PATH
try {
    $env:PATH="$env:SystemRoot\System32;$env:SystemRoot"
    $loader=Start-Process -FilePath (Join-Path $output 'Concat.exe') -ArgumentList '--version' -WindowStyle Hidden -PassThru
    if(!$loader.WaitForExit(15000)){$loader.Kill(); throw 'Packaged DLL-loader smoke timed out.'}
    if($loader.ExitCode-ne 0){throw "Packaged DLL-loader smoke failed: $($loader.ExitCode)"}
} finally {$env:PATH=$oldPath}
Write-Output "PACKAGE=$output"
Write-Output "BUILD_PROFILE=$profile"
Get-FileHash -LiteralPath (Join-Path $output 'Concat.exe') -Algorithm SHA256
