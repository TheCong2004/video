param([switch]$CheckOnly, [switch]$TestAdapters, [switch]$TestAI, [switch]$RunBuilt, [switch]$Build, [switch]$Release)

$ErrorActionPreference = 'Stop'
$concatRoot = Split-Path -Parent $PSScriptRoot
$sourceRoot = Join-Path $concatRoot 'src'
$depsRoot = Join-Path $concatRoot '.deps'
$names = @('FFMPEG_DIR', 'SHERPA_ONNX_LIB_DIR', 'ORT_LIB_LOCATION', 'PATH', 'CARGO_BUILD_JOBS', 'TEMP', 'TMP')
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }

function Find-SingleDirectory([string]$Pattern, [string]$Description) {
    $candidateDirs = @(Get-ChildItem -Path $Pattern -Directory -ErrorAction SilentlyContinue)
    if ($candidateDirs.Count -ne 1) { throw "Expected one $Description directory. Configure its environment variable explicitly." }
    return $candidateDirs[0].FullName
}

try {
    $chosenCount = [int]$CheckOnly.IsPresent + [int]$TestAdapters.IsPresent + [int]$TestAI.IsPresent + [int]$RunBuilt.IsPresent + [int]$Build.IsPresent + [int]$Release.IsPresent
    if ($chosenCount -gt 1) { throw 'Choose only one of -CheckOnly, -TestAdapters, -TestAI, -RunBuilt, -Build, or -Release.' }
    # Also prune before starting: a previous interrupted build may have left D full.
    try { & (Join-Path $PSScriptRoot 'cleanup-build.ps1') -Apply }
    catch { Write-Warning "Pre-build cleanup was not completed: $($_.Exception.Message)" }
    # Native compiler/test scratch files belong beside the existing D: target,
    # not on the system drive. Keep current caches; prune expired own artifacts.
    $nativeTemp = Join-Path $concatRoot '.runtime\native-temp'
    New-Item -ItemType Directory -Path $nativeTemp -Force | Out-Null
    $env:TEMP = $nativeTemp
    $env:TMP = $nativeTemp
    if (Test-Path Env:DOCS_RS) { throw 'Unset DOCS_RS before checking or running a native desktop build.' }
    if (!$env:FFMPEG_DIR) { $env:FFMPEG_DIR = Find-SingleDirectory (Join-Path $depsRoot 'ffmpeg\*') 'FFmpeg' }
    if (!$env:SHERPA_ONNX_LIB_DIR) { $env:SHERPA_ONNX_LIB_DIR = Join-Path $depsRoot 'sherpa-libs' }
    if (!$env:ORT_LIB_LOCATION) { $env:ORT_LIB_LOCATION = Find-SingleDirectory (Join-Path $depsRoot 'ort\*\lib') 'ONNX Runtime lib' }
    foreach ($name in @('FFMPEG_DIR', 'SHERPA_ONNX_LIB_DIR', 'ORT_LIB_LOCATION')) {
        $value = [Environment]::GetEnvironmentVariable($name, 'Process')
        if (!(Test-Path -LiteralPath $value -PathType Container)) { throw "$name directory is missing." }
    }
    # SHERPA_ONNX_LIB_DIR can contain only import libraries. Locate the actual
    # matching runtime rather than silently accepting an empty sherpa-dlls dir.
    $sherpaRuntimeCandidates = @(
        $env:SHERPA_ONNX_LIB_DIR,
        (Join-Path $depsRoot 'sherpa-dlls')
    )
    $sherpaRuntimeCandidates += @(Get-ChildItem -Path (Join-Path $depsRoot 'sherpa-onnx-v1.13.7-*-shared-*-lib\lib') -Directory -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName })
    $sherpaRuntime = $sherpaRuntimeCandidates | Where-Object {
        Test-Path -LiteralPath (Join-Path $_ 'sherpa-onnx-c-api.dll') -PathType Leaf
    } | Select-Object -First 1
    if (!$sherpaRuntime) { throw 'Missing sherpa-onnx-c-api.dll. Install the matching Sherpa 1.13.7 runtime; import .lib files alone are insufficient.' }
    $runtimeDirs = @((Join-Path $env:FFMPEG_DIR 'bin'), $env:ORT_LIB_LOCATION, $sherpaRuntime)
    $existingRuntimeDirs = @($runtimeDirs | Where-Object { Test-Path -LiteralPath $_ -PathType Container })
    $env:PATH = ($existingRuntimeDirs -join ';') + ';' + $env:PATH
    if (!$env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '2' }
    Push-Location -LiteralPath $sourceRoot
    try {
        # sherpa-onnx-sys 1.13.7 assumes a standard debug/release output folder.
        # Do not use Concat's custom quick profile until that upstream issue is fixed.
        if ($TestAdapters) {
            # Studio tests do not link the editor, media or speech engines.
            & cargo test -p concat-studio --profile quick
        }
        elseif ($TestAI) {
            & cargo test -p concat-agent -p concat-project --profile quick
            if ($LASTEXITCODE -ne 0) { throw "AI/project tests failed with exit code $LASTEXITCODE." }
            & cargo test -p concat-host --lib session::tests
            if ($LASTEXITCODE -ne 0) { throw "Native Session tests failed with exit code $LASTEXITCODE." }
            & cargo test -p concat-host --lib session::safety_tests
            if ($LASTEXITCODE -ne 0) { throw "Session safety tests failed with exit code $LASTEXITCODE." }
            & cargo test -p concat-host --lib contact_sheet::tests
            if ($LASTEXITCODE -ne 0) { throw "Contact sheet tests failed with exit code $LASTEXITCODE." }
            & cargo test -p concat-speech --lib transcript::tests
            if ($LASTEXITCODE -ne 0) { throw "Transcript tests failed with exit code $LASTEXITCODE." }
            & cargo test -p concat-speech --lib audio_analysis::tests
            if ($LASTEXITCODE -ne 0) { throw "Audio analysis tests failed with exit code $LASTEXITCODE." }
            & cargo test -p concat-api --profile quick
            if ($LASTEXITCODE -ne 0) { throw "Concat API and MCP bridge tests failed with exit code $LASTEXITCODE." }
        }
        elseif ($CheckOnly) { & cargo check -p concat }
        elseif ($Build) { & cargo build -p concat --bin concat }
        elseif ($Release) { & cargo build -p concat --bin concat --release }
        elseif ($RunBuilt) {
            $builtExe = Join-Path $sourceRoot 'target\debug\concat.exe'
            if (!(Test-Path -LiteralPath $builtExe -PathType Leaf)) { throw 'No built debug executable. Run the script without -RunBuilt to build it first.' }
            & $builtExe
        }
        else { & cargo run -p concat --bin concat }
        if ($LASTEXITCODE -ne 0) { throw "Concat native command failed with exit code $LASTEXITCODE." }
    } finally { Pop-Location }
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
    # Run on success OR failure. Cleanup never hides the original build result,
    # removes third-party libraries, or races another Cargo/test process.
    try { & (Join-Path $PSScriptRoot 'cleanup-build.ps1') -Apply }
    catch { Write-Warning "Build cleanup was not completed: $($_.Exception.Message)" }
}
