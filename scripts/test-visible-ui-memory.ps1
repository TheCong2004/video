param([switch]$KeepBinary)
$ErrorActionPreference='Stop'
$repo=Split-Path -Parent $PSScriptRoot
$sourceRoot=Join-Path $repo 'src'
$runtime=Join-Path $repo '.runtime'
$deps=Join-Path $sourceRoot 'target\debug\deps'
$testSource=Join-Path $PSScriptRoot 'tests\visible-ui-memory.rs'
$exe=Join-Path $runtime 'visible-ui-memory-test.exe'
$buildLog=Join-Path $runtime 'ai-control-visible-memory-build.log'
$testLog=Join-Path $runtime 'ai-control-visible-memory-tests.log'
function Latest-Library([string]$name) {
    $file=Get-ChildItem -LiteralPath $deps -Filter "lib$name-*.rlib" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if(!$file){throw "Compile the native app first: missing $name"}
    return $file.FullName
}
$ui=Latest-Library 'concat_ui'
$uiHash=([IO.Path]::GetFileNameWithoutExtension($ui)).Substring('libconcat_ui-'.Length)
$fingerprint=Join-Path $sourceRoot "target\debug\.fingerprint\concat-ui-$uiHash\lib-concat_ui.json"
$metadata=Get-Content -LiteralPath $fingerprint -Raw | ConvertFrom-Json
$slintDependency=@($metadata.deps | Where-Object {$_[1] -eq 'slint'})[0]
$expected=([BitConverter]::ToString([BitConverter]::GetBytes([UInt64]$slintDependency[3]))).Replace('-','').ToLowerInvariant()
$slint=$null
foreach($entry in Get-ChildItem (Join-Path $sourceRoot 'target\debug\.fingerprint') -Directory -Filter 'slint-*') {
    $stamp=Join-Path $entry.FullName 'lib-slint'
    $library=Join-Path $deps ('lib'+$entry.Name+'.rlib')
    if((Test-Path -LiteralPath $stamp) -and (Test-Path -LiteralPath $library) -and (Get-Content -LiteralPath $stamp -Raw).Trim() -eq $expected){$slint=$library;break}
}
if(!$slint){throw 'Cannot identify the exact Slint instance used by the main UI'}
$arguments=@('--edition=2024','--test',$testSource,'-L',"dependency=$deps",'--extern',"slint=$slint",'--extern',"concat_ui=$ui")
foreach($name in @('concat_control_ui','concat_studio','serde_json')){$arguments+=@('--extern',"$name=$(Latest-Library $name)")}
$native=@(Get-ChildItem (Join-Path $sourceRoot 'target\debug\build') -Recurse -File -Filter '*.lib' | ForEach-Object DirectoryName | Sort-Object -Unique)
$registry=Join-Path $env:USERPROFILE '.cargo\registry\src'
foreach($index in Get-ChildItem -LiteralPath $registry -Directory){$native+=@(Get-ChildItem -LiteralPath $index.FullName -Directory -Filter 'windows_x86_64_msvc-*' | ForEach-Object {Join-Path $_.FullName 'lib'})}
foreach($dir in $native){$arguments+=@('-L',"native=$dir")}
$arguments+=@('-C','debuginfo=0','-C','link-arg=/DEBUG:NONE','-o',$exe)
$previousTemp=$env:TEMP;$previousTmp=$env:TMP
Push-Location $sourceRoot
try {
    $env:TEMP=Join-Path $runtime 'native-temp';$env:TMP=$env:TEMP
    $ErrorActionPreference='Continue'
    & rustc @arguments *> $buildLog
    $compileExit=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    if($compileExit -ne 0){Get-Content -LiteralPath $buildLog -Tail 20;throw 'Headless component integration did not compile'}
    $ErrorActionPreference='Continue'
    & $exe --nocapture *> $testLog
    $testExit=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    Get-Content -LiteralPath $testLog -Tail 18
    if($testExit -ne 0){throw 'Headless component integration failed'}
} finally {
    Pop-Location
    $env:TEMP=$previousTemp;$env:TMP=$previousTmp
    if(!$KeepBinary -and (Test-Path -LiteralPath $exe)) {
        $resolved=(Resolve-Path -LiteralPath $exe).Path
        if($resolved -ne [IO.Path]::GetFullPath((Join-Path $runtime 'visible-ui-memory-test.exe'))){throw 'Unexpected test cleanup target'}
        Remove-Item -LiteralPath $resolved -Force
    }
}
