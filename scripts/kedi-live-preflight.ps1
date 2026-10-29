param([string]$AssetRoot,[string]$AvatarManifest,[string]$Ffprobe)
$ErrorActionPreference='Stop'
$checks=[Collections.Generic.List[object]]::new()
function Check([string]$Name,[bool]$Pass,[string]$Detail){$checks.Add([pscustomobject]@{name=$Name;passed=$Pass;detail=$Detail})}
Check 'platform-writes-off' ($env:KEDI_PLATFORM_WRITE_ENABLED -notmatch '^(true|1|on)$') 'Public platform actions remain disabled until staging approval.'
$gpu=Get-Command nvidia-smi -ErrorAction SilentlyContinue
if($gpu){$info=& $gpu.Source --query-gpu=name,memory.total --format=csv,noheader 2>$null;Check 'gpu-present' ($LASTEXITCODE -eq 0) ($info -join '; ')}else{Check 'gpu-present' $false 'nvidia-smi unavailable; GPU live smoke not run.'}
if(!$Ffprobe){$probe=Get-Command ffprobe -ErrorAction SilentlyContinue;if($probe){$Ffprobe=$probe.Source}}
Check 'ffprobe-present' ([bool]($Ffprobe -and (Test-Path -LiteralPath $Ffprobe -PathType Leaf))) 'Use the approved bundled FFmpeg tools.'
if($AvatarManifest -and $AssetRoot){
    $root=(Resolve-Path -LiteralPath $AssetRoot).Path
    $manifest=Get-Content -LiteralPath $AvatarManifest -Raw -Encoding UTF8 | ConvertFrom-Json
    Check 'consent-and-rights' ([bool]($manifest.consent_ref -and $manifest.commercial_allowed -and ([DateTimeOffset]::Parse($manifest.rights_expires_at) -gt [DateTimeOffset]::UtcNow))) 'Operator must verify the referenced consent record, source footage and voice rights.'
    foreach($clip in $manifest.motions.clips){
        $path=[IO.Path]::GetFullPath((Join-Path $root $clip.path))
        $contained=$path.StartsWith($root.TrimEnd('\')+'\',[StringComparison]::OrdinalIgnoreCase)
        if(!$contained -or !(Test-Path -LiteralPath $path -PathType Leaf)){Check "asset:$($clip.clip_id)" $false 'Path missing or outside approved root';continue}
        $hash=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
        Check "hash:$($clip.clip_id)" ($hash -eq $clip.sha256) 'SHA-256 content pin'
        if($Ffprobe){$raw=& $Ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,width,height -show_entries format=duration -of json $path 2>$null;if($LASTEXITCODE -ne 0){Check "decode:$($clip.clip_id)" $false 'ffprobe rejected asset';continue};$media=($raw -join '')|ConvertFrom-Json;$v=$media.streams|Select-Object -First 1;Check "decode:$($clip.clip_id)" ([bool]($v -and $v.width -gt 0 -and $v.height -gt 0 -and [double]$media.format.duration -ge 1 -and [double]$media.format.duration -le 10.05)) 'Prepared clip must contain video and run 1–10 seconds.'}
    }
}else{Check 'avatar-assets' $false 'Supply -AssetRoot and -AvatarManifest for real preview preflight.'}
$checks|ConvertTo-Json -Depth 5
if(@($checks|Where-Object{!$_.passed}).Count){exit 2}
