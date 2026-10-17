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
$output=Join-Path $root ('dist\windows\ConcatStudio-LiveMotion-'+(Get-Date -Format 'yyyyMMdd-HHmmss')+'.zip')
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$partial=$output+'.partial'
$file=[IO.File]::Open($partial,[IO.FileMode]::CreateNew)
$zip=[IO.Compression.ZipArchive]::new($file,[IO.Compression.ZipArchiveMode]::Create)
$prefix='ConcatStudio-LiveMotion/'
try {
    # Stream directly from the already verified runtime. Do not create another
    # multi-GB staging directory, and never copy a user's per-account data.
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
    [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip,(Join-Path $root 'docs\LIVE_MOTION.md'),$prefix+'LIVE_MOTION.md',[IO.Compression.CompressionLevel]::Optimal)
} finally {
    $zip.Dispose()
    $file.Dispose()
}
# Read the actual compressed executable back, not only the input on disk.
$read=[IO.Compression.ZipFile]::OpenRead($partial)
try {
    $entry=$read.GetEntry($prefix+'Concat.exe')
    $stream=$entry.Open()
    $sha=[Security.Cryptography.SHA256]::Create()
    try {$inside=([BitConverter]::ToString($sha.ComputeHash($stream))).Replace('-','')}
    finally {$stream.Dispose();$sha.Dispose()}
    $expected=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
    if($inside-ne $expected){throw 'Packaged executable hash mismatch.'}
} finally {$read.Dispose()}
[IO.File]::Move($partial,$output)
Write-Output "PORTABLE_ZIP=$output"
Write-Output "EXE_SHA256=$expected"
Get-Item -LiteralPath $output | Select-Object FullName,Length
