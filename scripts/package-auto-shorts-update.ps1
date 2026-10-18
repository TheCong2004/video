param(
    [Parameter(Mandatory=$true)][string]$ExePath,
    [Parameter(Mandatory=$true)][string]$RuntimePackage
)
$ErrorActionPreference='Stop'
$root=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$exe=(Resolve-Path -LiteralPath $ExePath).Path
$runtime=(Resolve-Path -LiteralPath $RuntimePackage).Path
foreach($required in @('avcodec-62.dll','onnxruntime.dll','resources\node\node.exe','resources\tiktok-live-connector\dist\artcraftLiveRuntime.js','OmniRoute\server.js')){
    if(!(Test-Path -LiteralPath (Join-Path $runtime $required) -PathType Leaf)){throw "Incomplete runtime package: $required"}
}
$outDir=Join-Path $root 'dist\windows'
New-Item -ItemType Directory -Path $outDir -Force | Out-Null
$output=Join-Path $outDir ('ConcatStudio-AutoShorts-'+(Get-Date -Format 'yyyyMMdd-HHmmss')+'.zip')
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$partial=$output+'.partial'
$file=[IO.File]::Open($partial,[IO.FileMode]::CreateNew)
$zip=[IO.Compression.ZipArchive]::new($file,[IO.Compression.ZipArchiveMode]::Create)
$prefix='ConcatStudio-AutoShorts/'
try {
    $count=0
    foreach($item in Get-ChildItem -LiteralPath $runtime -Recurse -File){
        if(($item.Attributes-band [IO.FileAttributes]::ReparsePoint)-ne 0){throw 'Refusing a linked package file.'}
        if($item.Name-match '(^\.env($|\.)|\.(dpapi|pdb|db|sqlite|sqlite3|log)(-|$))'){continue}
        $relative=$item.FullName.Substring($runtime.Length+1).Replace('\','/')
        if($relative-eq 'Concat.exe' -or $relative-eq 'LIVE_MOTION.md'){continue}
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$item.FullName,$prefix+$relative,[IO.Compression.CompressionLevel]::Fastest)
        $count++
        if($count%5000-eq 0){Write-Output "PACKAGED_FILES=$count"}
    }
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$exe,$prefix+'Concat.exe',[IO.Compression.CompressionLevel]::Optimal)
    foreach($relative in @('docs\AUTO_SHORTS.md','docs\HOTCLIP_VERIFICATION.md','docs\ai-port\FEATURE_PARITY_MATRIX.md','THIRD_PARTY_NOTICES.md')){
        $path=Join-Path $root $relative
        [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,$path,$prefix+('docs/'+[IO.Path]::GetFileName($relative)),[IO.Compression.CompressionLevel]::Optimal)
    }
} finally {
    $zip.Dispose()
    $file.Dispose()
}
$read=[IO.Compression.ZipFile]::OpenRead($partial)
try {
    $entry=$read.GetEntry($prefix+'Concat.exe')
    if(!$entry){throw 'Portable package does not contain Concat.exe.'}
    $stream=$entry.Open()
    $sha=[Security.Cryptography.SHA256]::Create()
    try {$inside=([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','')}
    finally {$stream.Dispose();$sha.Dispose()}
    $expected=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    if($inside-ne $expected){throw 'Packaged executable hash mismatch.'}
    foreach($required in @('Concat.exe','resources/node/node.exe','OmniRoute/server.js','docs/AUTO_SHORTS.md','docs/THIRD_PARTY_NOTICES.md')){
        if(!$read.GetEntry($prefix+$required)){throw "Portable package is missing $required"}
    }
} finally {$read.Dispose()}
[IO.File]::Move($partial,$output)
Write-Output "PORTABLE_ZIP=$output"
Write-Output "EXE_SHA256=$expected"
Get-Item -LiteralPath $output | Select-Object FullName,Length
