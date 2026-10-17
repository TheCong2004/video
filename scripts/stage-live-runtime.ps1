param(
    [string]$SourceRoot='D:\capcutpolot\artcraft\crates\desktop\artcraft\resources\tiktok-live-connector',
    [string]$ConnectorSourceRoot='D:\capcutpolot\artcraft\tools\tiktok-live-connector'
)
$ErrorActionPreference='Stop'
$concatRoot=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$source=(Resolve-Path -LiteralPath $SourceRoot).Path
foreach($required in @('dist\artcraftLiveRuntime.js','node_modules','package.json','LICENSE','THIRD_PARTY_NOTICES.md','sbom.spdx.json')) {
    if(!(Test-Path -LiteralPath (Join-Path $source $required))){throw "Incomplete TikTok runtime: $required"}
}
$target=Join-Path $concatRoot '.deps\tiktok-live-connector'
New-Item -ItemType Directory -Path $target -Force | Out-Null
foreach($name in @('package.json','LICENSE','README.md','THIRD_PARTY_NOTICES.md','sbom.spdx.json')) {
    Copy-Item -LiteralPath (Join-Path $source $name) -Destination $target -Force
}
foreach($name in @('dist','node_modules')) {
    & robocopy (Join-Path $source $name) (Join-Path $target $name) /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XF '.env' '.env.*' '*.db' '*.db-*' '*.log' /XD '.git' 'test' 'tests' | Out-Null
    if($LASTEXITCODE -ge 8){throw "TikTok runtime staging failed: $name"}
}
# Ship the actual modified connector sources alongside its executable runtime,
# rather than only referring users to an older upstream source revision.
if(!(Test-Path -LiteralPath (Join-Path $ConnectorSourceRoot 'src\artcraft-live-runtime.ts'))){throw 'Missing corresponding connector sources.'}
$corresponding=Join-Path $target 'corresponding-source'
New-Item -ItemType Directory -Path $corresponding -Force | Out-Null
foreach($name in @('src','test','scripts','legal','package.json','pnpm-lock.yaml','pnpm-workspace.yaml','tsconfig.json','tsconfig.test.json','tsdown.config.ts','vitest.config.ts','LICENSE','README.md')) {
    Copy-Item -LiteralPath (Join-Path $ConnectorSourceRoot $name) -Destination $corresponding -Recurse -Force
}
Write-Output "LIVE_RUNTIME=$target"
