# Machine integration regression: installed native dependencies are required.
# Cargo is replaced by a function so this test never builds or launches an app.
$ErrorActionPreference = 'Stop'
$launcher = Join-Path $PSScriptRoot 'windows-studio.ps1'
$before = @{}
$variableNames = @('PATH', 'FFMPEG_DIR', 'SHERPA_ONNX_LIB_DIR', 'ORT_LIB_LOCATION', 'CARGO_BUILD_JOBS')
foreach ($name in $variableNames) { $before[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
$global:ConcatLauncherTestCalls = 0
function cargo {
    if (($args -join ' ') -ne 'check -p concat') { throw 'Unexpected Cargo invocation in launcher test.' }
    foreach ($dll in @('sherpa-onnx-c-api.dll', 'onnxruntime.dll', 'avcodec-62.dll')) {
        $resolved = @($env:PATH.Split(';') | Where-Object {
            $_ -and (Test-Path -LiteralPath (Join-Path $_ $dll) -PathType Leaf)
        })
        if (!$resolved.Count) { throw "Launcher PATH does not resolve $dll." }
    }
    $global:ConcatLauncherTestCalls++
    $global:LASTEXITCODE = 0
}
try {
    & $launcher -CheckOnly
    if ($global:ConcatLauncherTestCalls -ne 1) { throw 'Cargo was not called exactly once.' }
    foreach ($name in $variableNames) {
        if ([Environment]::GetEnvironmentVariable($name, 'Process') -cne $before[$name]) {
            throw "Launcher did not restore $name."
        }
    }
    Write-Output 'PASS: native DLLs resolve; check command is preserved; process environment restored.'
} finally {
    Remove-Item Function:cargo -ErrorAction SilentlyContinue
    Remove-Variable ConcatLauncherTestCalls -Scope Global -ErrorAction SilentlyContinue
}
