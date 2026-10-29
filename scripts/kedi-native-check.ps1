param([string]$PreviewExecutable, [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
if (!$PreviewExecutable) { $PreviewExecutable = Join-Path $repo '.runtime\nexpulse-build\debug\concat-studio-preview.exe' }
if (!$OutputDirectory) { $OutputDirectory = Join-Path $repo '.runtime\kedi-ui-20261022' }
$exe = (Resolve-Path -LiteralPath $PreviewExecutable).Path
New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$failures = @()
$renderCount = 0
$widths = @(320,360,560,680,940,960,1280,1440,1920)
foreach ($theme in @('light','dark')) {
    # Every native Studio route at the representative desktop viewport.
    foreach ($route in @('production','live','mc','auto-shorts','scripts','keys','settings','channels','bulk','publish','workflows','jobs','history')) {
        & $exe (Join-Path $OutputDirectory "$route-$theme-960.png") $route 960 900 $theme
        if ($LASTEXITCODE -ne 0) { $failures += "$route/$theme/960" } else { $renderCount++ }
    }
    # Dense forms plus the actual launcher at each requested width.
    foreach ($width in $widths) {
        foreach ($route in @('production','live','kedi-sidebar')) {
            $nativeOutput = & $exe (Join-Path $OutputDirectory "$route-$theme-$width.png") $route $width 900 $theme
            $nativeExit = $LASTEXITCODE
            $nativeOutput | Write-Output
            $receiptMissing = $route -eq 'kedi-sidebar' -and !($nativeOutput -match 'KEDI_SIDEBAR=PASS')
            if ($nativeExit -ne 0 -or $receiptMissing) { $failures += "$route/$theme/$width" } else { $renderCount++ }
        }
    }
}
"KEDI_NATIVE_RENDERS=$renderCount; FAILURES=$($failures.Count); OUTPUT=$OutputDirectory"
$motionOutput = & $exe (Join-Path $OutputDirectory 'motion-bindings.png') settings 960 900 kedi-motion
$motionExit = $LASTEXITCODE
$motionOutput | Write-Output
if ($motionExit -ne 0 -or !($motionOutput -match 'KEDI_MOTION=PASS')) { $failures += 'native motion bindings' }
if ($failures.Count) { throw ('Native UI checks failed: ' + ($failures -join ', ')) }
'KEDI_NATIVE_CHECK=PASS (native software renders and actual sidebar pointer/key tests; not OS DPI or GPU/playback benchmarks)'
