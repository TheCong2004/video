param([string]$SourceRoot='D:\capcutpolot\artcraft\crates\desktop\artcraft\resources\OmniRoute')
$ErrorActionPreference='Stop'
$concatRoot=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$source=(Resolve-Path -LiteralPath $SourceRoot).Path
foreach($required in @('node.exe','server.js','.build\next\BUILD_ID')) {
    if(!(Test-Path -LiteralPath (Join-Path $source $required) -PathType Leaf)){throw "Incomplete standalone runtime: $required"}
}
$target=Join-Path $concatRoot '.deps\omniroute'
New-Item -ItemType Directory -Path $target -Force | Out-Null
# Runtime allowlist, never .env, local databases or user configuration stores.
foreach($name in @('node.exe','server.js','package.json')) {Copy-Item -LiteralPath (Join-Path $source $name) -Destination $target -Force}
foreach($name in @('.build','node_modules','public','open-sse','src','skills','config')) {
    $dir=Join-Path $source $name
    if(!(Test-Path -LiteralPath $dir)){continue}
    & robocopy $dir (Join-Path $target $name) /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XF '.env' '.env.*' '*.db' '*.db-*' '*.sqlite' '*.sqlite-*' '*.sqlite3' '*.sqlite3-*' '*.log' /XD '.git' 'tests' 'test' 'logs' 'data' | Out-Null
    if($LASTEXITCODE -ge 8){throw "Runtime staging failed: $name"}
}
& (Join-Path $target 'node.exe') (Join-Path $PSScriptRoot 'harden-local-ai.mjs') $target
if($LASTEXITCODE -ne 0){throw 'OmniRoute auth hardening failed; do not package this runtime.'}
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'local-ai-LICENSE.txt') -Destination (Join-Path $target 'LICENSE') -Force
Write-Output "LOCAL_AI_RUNTIME=$target"
