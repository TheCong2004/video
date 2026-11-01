param([switch]$Version,[switch]$Smoke,[switch]$UseExistingProfile,[switch]$Wait,[switch]$Preview,[switch]$PreviousBuild,[string]$CandidateName,[switch]$RecoverWindowless,[switch]$LiveStudioSmoke)
$ErrorActionPreference='Stop'
if($Version -and $Smoke){throw 'Choose -Version or -Smoke.'}
if($LiveStudioSmoke -and !$Smoke){throw '-LiveStudioSmoke requires -Smoke and an isolated test profile.'}
if($LiveStudioSmoke -and $UseExistingProfile){throw 'Live Studio smoke cannot use the existing user profile.'}
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
# Only a smoke-checked, hash-bound publication can change the normal launcher.
. (Join-Path $PSScriptRoot 'select-kedi-build.ps1')
$selected=Resolve-KediBuild -Repo $repo -UseExistingProfile:$UseExistingProfile -Preview:$Preview -PreviousBuild:$PreviousBuild -CandidateName $CandidateName
$folder=$selected.Folder
$exe=$selected.Executable
if($selected.VerifiedRevision){
    if(!$Version -and !$Smoke){
    $running=@(Get-Process -ErrorAction SilentlyContinue | Where-Object {$_.ProcessName -like 'concat*' -and $_.ProcessName -ne 'concat-campaign-worker' -and $_.Path -and ([IO.Path]::GetDirectoryName($_.Path) -eq $folder)})
        if($running.Count){
            $current=@($running | Where-Object {$_.Path -eq $exe})
            if($current.Count){
                $activated=$false
                $shell=New-Object -ComObject WScript.Shell
                try {
                    foreach($instance in $current){
                        if($shell.AppActivate([int]$instance.Id)){$activated=$true;break}
                    }
                } finally {[void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell)}
                if($activated){Write-Output 'Kedi is already running the selected build. Switched to its existing window.'}
                elseif($RecoverWindowless){
                    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class KediLauncherWindows {
    public delegate bool Callback(IntPtr window, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumWindows(Callback callback, IntPtr data);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    public static bool HasWindow(uint process) {
        bool found=false;
        if(!EnumWindows((window,data)=>{uint owner;GetWindowThreadProcessId(window,out owner);if(owner==process)found=true;return true;},IntPtr.Zero))
            throw new InvalidOperationException("Cannot inspect Kedi windows.");
        return found;
    }
}
'@
                    foreach($instance in $current){
                        $instance.Refresh()
                        if($instance.HasExited){continue}
                        if(([DateTime]::Now-$instance.StartTime).TotalSeconds -lt 30 -or [KediLauncherWindows]::HasWindow([uint32]$instance.Id)){
                            throw 'Kedi is starting or still owns a window. Recovery will not close it.'
                        }
                        Write-Output "Recovering Kedi PID $($instance.Id): no top-level window remains."
                        Stop-Process -InputObject $instance
                        if(!$instance.WaitForExit(10000)){throw 'Kedi did not exit; refusing to launch a duplicate.'}
                    }
                    $recoveredIds=@($current | ForEach-Object Id)
                    $running=@($running | Where-Object {$_.Id -notin $recoveredIds})
                }
                else {Write-Output 'Kedi could not show a window. To recover a process with no windows, run: powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\run-kedi-test.ps1 -UseExistingProfile -RecoverWindowless'}
            }else{
                Write-Output 'An older Kedi build is still open. Save your work and close both the main and AI Control windows, then run again to use the selected build.'
            }
            if($running.Count){
                Write-Output ('Running Kedi PID: '+(($running | ForEach-Object Id) -join ', '))
                $global:LASTEXITCODE=0
                return
            }
        }
    }
    Write-Output "Verified build: $($selected.VerifiedRevision)"
}
$ffmpeg=@(Get-Item -Path (Join-Path $repo '.deps\ffmpeg\*\bin')|Where-Object PSIsContainer)
$ort=@(Get-Item -Path (Join-Path $repo '.deps\ort\*\lib')|Where-Object PSIsContainer)
$sherpa=@((Join-Path $repo '.deps\sherpa-dlls'))
$sherpa+=@(Get-Item -Path (Join-Path $repo '.deps\sherpa-onnx-*-shared-*-lib\lib') | Where-Object PSIsContainer | ForEach-Object FullName)
$sherpa=$sherpa|Where-Object{Test-Path -LiteralPath (Join-Path $_ 'sherpa-onnx-c-api.dll')}|Select-Object -First 1
if($ffmpeg.Count -ne 1 -or $ort.Count -ne 1 -or !$sherpa){throw 'Required FFmpeg/ONNX/Sherpa runtime is missing or ambiguous.'}
# A test-only portable profile keeps existing user settings and accounts isolated.
if(!$UseExistingProfile){New-Item -ItemType Directory -Path (Join-Path $folder 'portable') -Force|Out-Null}
$info=[Diagnostics.ProcessStartInfo]::new()
$info.FileName=$exe
$info.WorkingDirectory=$repo
$info.UseShellExecute=$false
$info.EnvironmentVariables['PATH']=$ffmpeg[0].FullName+';'+$ort[0].FullName+';'+$sherpa+';'+$env:PATH
$info.EnvironmentVariables['KEDI_LIVE_ENABLED']='false'
$info.EnvironmentVariables['FFMPEG_DIR']=$ffmpeg[0].Parent.FullName
$info.EnvironmentVariables['KEDI_OT_ADAPTER_ENABLED']='false'
$info.EnvironmentVariables['KEDI_COMMERCE_ENABLED']='false'
$info.EnvironmentVariables['KEDI_MOTION_ENABLED']='false'
$info.EnvironmentVariables['KEDI_PLATFORM_WRITE_ENABLED']='false'
if($env:CONCAT_POSTIZ_MODE){$info.EnvironmentVariables['CONCAT_POSTIZ_MODE']=$env:CONCAT_POSTIZ_MODE}
if($env:POSTIZ_DEV_URL){$info.EnvironmentVariables['POSTIZ_DEV_URL']=$env:POSTIZ_DEV_URL}
foreach($name in @($info.EnvironmentVariables.Keys)){if($name -match '^CONCAT_.*SMOKE'){[void]$info.EnvironmentVariables.Remove($name)}}
if($LiveStudioSmoke){$info.EnvironmentVariables['CONCAT_KEDI_OPERATOR_SMOKE']='1'}
if($Version -or $Smoke){
    $info.CreateNoWindow=$true
    $info.WindowStyle=[Diagnostics.ProcessWindowStyle]::Hidden
    $info.RedirectStandardOutput=$true
    $info.RedirectStandardError=$true
    $info.Arguments=if($Version){'--version'}else{'--smoke'}
}else{$info.WindowStyle=[Diagnostics.ProcessWindowStyle]::Normal}
Write-Output "Executable: $exe"
$process=[Diagnostics.Process]::Start($info)
if(!$process){throw 'Could not start the test executable.'}
if($Version -or $Smoke){
    $stdout=$process.StandardOutput.ReadToEndAsync()
    $stderr=$process.StandardError.ReadToEndAsync()
    $deadline=[DateTime]::UtcNow.AddSeconds(90)
    while(!$process.WaitForExit(1000)){if([DateTime]::UtcNow -gt $deadline){$process.Kill();throw 'Owned smoke/version process timed out.'}}
    $stdout.Result|Write-Output
    if($stderr.Result){Write-Output $stderr.Result}
    Write-Output "KEDI_TEST_PROCESS_EXIT=$($process.ExitCode)"
    exit $process.ExitCode
}
Write-Output "Kedi started (PID $($process.Id))."
$global:LASTEXITCODE=0
if($Wait){
    $process.WaitForExit()
    $global:LASTEXITCODE=$process.ExitCode
    if($process.ExitCode -ne 0){throw "Kedi exited with code $($process.ExitCode)."}
}
