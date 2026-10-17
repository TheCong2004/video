param([Parameter(Mandatory=$true)][string]$PackageDirectory, [string]$NsisCompiler)
$ErrorActionPreference='Stop'
$concatRoot=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$package=(Resolve-Path -LiteralPath $PackageDirectory).Path.TrimEnd('\')
foreach($file in @('Concat.exe','OmniRoute\server.js','resources\node\node.exe','resources\tiktok-live-connector\dist\artcraftLiveRuntime.js')) {
    if(!(Test-Path -LiteralPath (Join-Path $package $file) -PathType Leaf)){throw "Incomplete package: $file"}
}
if(!$NsisCompiler) {
    $compilerCandidates=@(
        (Join-Path $env:LOCALAPPDATA 'tauri\NSIS\Bin\makensis.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'NSIS\makensis.exe')
    )
    $NsisCompiler=$compilerCandidates | Where-Object {Test-Path -LiteralPath $_ -PathType Leaf} | Select-Object -First 1
}
if(!$NsisCompiler){throw 'NSIS compiler not installed. Portable package remains available.'}
$output=Join-Path $concatRoot ('dist\windows\ConcatStudio-Setup-x64-'+(Get-Date -Format 'yyyyMMdd-HHmmss')+'.exe')
if(Test-Path -LiteralPath $output){throw 'Installer already exists; refusing overwrite.'}
$scratch=Join-Path $concatRoot ('.runtime\installer-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $scratch -ErrorAction Stop | Out-Null
$items=@(Get-ChildItem -LiteralPath $package -Recurse -Force)
if(@($items | Where-Object {$_.Attributes -band [IO.FileAttributes]::ReparsePoint}).Count){throw 'Refusing installer input with reparse points.'}
function Escape-Nsis([string]$Value){return $Value.Replace('$','$$').Replace('"','$\"')}
$lines=[Collections.Generic.List[string]]::new()
foreach($item in @($items | Where-Object {!$_.PSIsContainer})){
    $relative=$item.FullName.Substring($package.Length+1)
    $lines.Add('Delete "$INSTDIR\'+(Escape-Nsis $relative)+'"')
}
foreach($dir in @($items | Where-Object {$_.PSIsContainer} | Sort-Object {$_.FullName.Length} -Descending)){
    $relative=$dir.FullName.Substring($package.Length+1)
    $lines.Add('RMDir "$INSTDIR\'+(Escape-Nsis $relative)+'"')
}
$manifest=Join-Path $scratch 'remove-packaged-files.nsh'
# Generated build manifest, not user data or a source rewrite.
[IO.File]::WriteAllLines($manifest,$lines,(New-Object Text.UTF8Encoding($true)))
$previousTemp=$env:TEMP
$previousTmp=$env:TMP
try {
    # NSIS compression scratch must also stay on D, not the system drive.
    $env:TEMP=$scratch
    $env:TMP=$scratch
    & $NsisCompiler '/WX' '/V3' "/DPACKAGE_DIR=$package" "/DOUTPUT_FILE=$output" "/DREMOVE_MANIFEST=$manifest" (Join-Path $PSScriptRoot 'windows-installer.nsi')
    if($LASTEXITCODE -ne 0){throw 'Installer compilation failed.'}
} finally {
    $env:TEMP=$previousTemp
    $env:TMP=$previousTmp
}
Get-Item -LiteralPath $output | Select-Object FullName,Length
Get-FileHash -LiteralPath $output -Algorithm SHA256
try {
    # Exact generated manifest and then its empty directory only; never recurse
    # into package outputs, user projects, app settings, or browser profiles.
    Remove-Item -LiteralPath $manifest -ErrorAction Stop
    Remove-Item -LiteralPath $scratch -ErrorAction Stop
} catch {
    Write-Warning 'Installer succeeded; its build scratch was retained for inspection.'
}
