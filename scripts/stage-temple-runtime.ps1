param(
    [string]$Destination = (Join-Path $PSScriptRoot '..\src\target\debug\resources'),
    [string]$NodeDirectory = (Join-Path $PSScriptRoot '..\.deps\temple-live-node')
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$source = Join-Path $repo 'integrations\temple-live'
$live = Join-Path $source 'live'
if (!(Test-Path (Join-Path $live 'node_modules\ws\package.json'))) {
    throw 'Run npm ci --ignore-scripts in integrations/temple-live/live before staging.'
}
foreach ($file in @('node.exe', 'LICENSE')) {
    if (!(Test-Path (Join-Path $NodeDirectory $file))) { throw "Missing bundled Node $file in $NodeDirectory." }
}
$target = [IO.Path]::GetFullPath($Destination)
New-Item -ItemType Directory -Force -Path (Join-Path $target 'temple-live\live'), (Join-Path $target 'temple-live\node') | Out-Null
foreach ($file in @('server.mjs','tts-service.mjs','tiktok-events.mjs','studio.html','control.html','overlay.html','package.json','package-lock.json')) {
    Copy-Item -LiteralPath (Join-Path $live $file) -Destination (Join-Path $target 'temple-live\live') -Force
}
Copy-Item -LiteralPath (Join-Path $source 'assets') -Destination (Join-Path $target 'temple-live') -Recurse -Force
Copy-Item -LiteralPath (Join-Path $source 'LICENSE') -Destination (Join-Path $target 'temple-live') -Force
# Copy installed, lockfile-pinned dependencies only; no account/cache/data folders.
Copy-Item -LiteralPath (Join-Path $live 'node_modules') -Destination (Join-Path $target 'temple-live\live') -Recurse -Force
foreach ($file in @('node.exe','LICENSE')) {
    $sourceFile = Join-Path $NodeDirectory $file
    $targetFile = Join-Path $target "temple-live\node\$file"
    # An unchanged running Node executable is locked by Windows. Keep it;
    # rebuilding Concat must not interrupt an active OBS/temple server.
    if (!(Test-Path -LiteralPath $targetFile) -or (Get-FileHash -LiteralPath $sourceFile).Hash -ne (Get-FileHash -LiteralPath $targetFile).Hash) {
        Copy-Item -LiteralPath $sourceFile -Destination $targetFile -Force
    }
}
Write-Output "Staged Chanh Dien runtime: $target"
