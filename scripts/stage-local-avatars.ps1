param([Parameter(Mandatory=$true)][string]$Destination)
$ErrorActionPreference='Stop'
$concatRoot=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$target=[IO.Path]::GetFullPath($Destination)
if(Test-Path -LiteralPath $target){throw 'AvatarRuntime destination already exists; refusing to merge stale files.'}
$python='cpython-3.11.15-windows-x86_64-none'
foreach($required in @(".runtime/python/$python/python.exe",'.runtime/slive-venv/Lib/site-packages/torch/__init__.py','.runtime/slive-models/s3fd.pth','integrations/slive/runtime/avatar_server.py')) {
    if(!(Test-Path -LiteralPath (Join-Path $concatRoot $required))){throw 'Install the Concat avatar runtime first.'}
}
if((Get-FileHash -LiteralPath (Join-Path $concatRoot '.runtime/slive-models/s3fd.pth') -Algorithm SHA256).Hash -ne '619A31681264D3F7F7FC7A16A42CBBE8B23F31A256F75A366E5A1BCD59B33543'){throw 'Model integrity check failed.'}
New-Item -ItemType Directory -Path $target | Out-Null
# Explicit resources only. No user library, tokens, environment files or job data.
foreach($relative in @(".runtime/python/$python",'.runtime/slive-venv/Lib/site-packages','.runtime/slive-models','integrations/slive/upstream','integrations/slive/runtime')) {
    $output=Join-Path $target $relative
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    & robocopy (Join-Path $concatRoot $relative) $output /E /R:1 /W:1 /NFL /NDL /NJH /NJS /NP /XJ /XD '__pycache__' /XF '*.pyc' '.env' '.env.*' '*.log' | Out-Null
    if($LASTEXITCODE -ge 8){throw "Could not stage avatar resource: $relative"}
}
Copy-Item -LiteralPath (Join-Path $concatRoot 'integrations/slive/source-manifest.json') -Destination (Join-Path $target 'integrations/slive/source-manifest.json')
Write-Output "AVATAR_RUNTIME=$target"
