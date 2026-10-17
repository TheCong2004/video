param([Parameter(Mandatory=$true)][string]$Executable)
$ErrorActionPreference = 'Stop'
$smokeRoot = [IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$exe = (Resolve-Path -LiteralPath $Executable).Path
$scratch = Join-Path $smokeRoot ('.runtime\package-smoke-'+[Guid]::NewGuid().ToString('N'))
$names = @('APPDATA','LOCALAPPDATA','TEMP','TMP','PATH')
$previous = @{}
foreach($name in $names) {$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
try {
    foreach($name in @('APPDATA','LOCALAPPDATA','TEMP','TMP')) {
        $dir=Join-Path $scratch $name
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
        [Environment]::SetEnvironmentVariable($name,$dir,'Process')
    }
    $env:PATH="$env:SystemRoot\System32;$env:SystemRoot"
    # The app's own bounded smoke creates the GPU/engine/window models, never
    # shows a window, publishes once and exits. No account or provider is used.
    $proc=Start-Process -FilePath $exe -ArgumentList '--smoke' -WorkingDirectory (Split-Path -Parent $exe) -WindowStyle Hidden -PassThru
    if(!$proc.WaitForExit(55000)) {
        # Only the exact child created above; never touch an existing app.
        $proc.Kill()
        throw 'Native startup smoke timed out.'
    }
    if($proc.ExitCode -ne 0){throw "Native startup smoke failed: $($proc.ExitCode). Inspect $scratch"}
    Write-Output "NATIVE_STARTUP_SMOKE=PASS"
    Write-Output "ISOLATED_DATA=$scratch"
} finally {
    foreach($name in $names) {[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
