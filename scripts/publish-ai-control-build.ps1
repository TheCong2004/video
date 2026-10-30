param([ValidatePattern('^concat-ai-control-[a-z0-9-]+\.exe$')][string]$CandidateName='concat-ai-control-preview.exe',[ValidatePattern('^[a-z0-9-]+\.log$')][string]$BuildLogName='ai-control-cozy-native-build.log',[string]$UiRevision='Cozy AI workspace / 2026-10-28',[switch]$CampaignWorker,[ValidatePattern('^[a-z0-9-]+\.log$')][string]$WorkerBuildLogName='campaign-worker-build.log')
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$preview=Join-Path (Join-Path $repo 'dist\kedi-test') $CandidateName
$buildLog=Join-Path (Join-Path $repo '.runtime') $BuildLogName
if(!(Test-Path -LiteralPath $preview -PathType Leaf)){throw 'Preview executable is missing.'}
if(!(Select-String -LiteralPath $buildLog -Pattern 'Finished `dev` profile' -Quiet)){
    throw 'The Cozy native build has not finished successfully. Nothing was published.'
}
$workerSource=Join-Path $repo 'src\target\debug\concat-campaign-worker.exe'
if($CampaignWorker){
    $workerBuildLog=Join-Path (Join-Path $repo '.runtime') $WorkerBuildLogName
    if(!(Test-Path -LiteralPath $workerSource -PathType Leaf) -or !(Select-String -LiteralPath $workerBuildLog -Pattern 'Finished `dev` profile' -Quiet)) { throw 'Campaign worker build is missing or incomplete. Nothing was published.' }
    $workerVersion=& $workerSource --version 2>&1
    if($LASTEXITCODE -ne 0 -or $workerVersion -notmatch '^concat-campaign-worker [0-9.]+ protocol=1$') { throw 'Campaign worker version check failed.' }
}
foreach($check in @('Version','Smoke')){
    $global:LASTEXITCODE=0
    $argsCheck=@{Preview=$true;CandidateName=$CandidateName}
    $argsCheck[$check]=$true
    $checkLog=Join-Path $repo ('.runtime\'+$BuildLogName.Replace('-native-build.log','')+'-'+$check.ToLowerInvariant()+'.log')
    & (Join-Path $PSScriptRoot 'run-kedi-test.ps1') @argsCheck *> $checkLog
    if($LASTEXITCODE -ne 0){Get-Content -LiteralPath $checkLog -Tail 15;throw "$check failed; nothing was published."}
    if(!(Select-String -LiteralPath $checkLog -Pattern '^KEDI_TEST_PROCESS_EXIT=0$' -Quiet)){throw 'Missing successful process receipt.'}
}
$hash=(Get-FileHash -LiteralPath $preview -Algorithm SHA256).Hash.ToLowerInvariant()
$name="concat-ai-control-$($hash.Substring(0,12)).exe"
$destination=Join-Path (Join-Path $repo 'src\target\debug') $name
if(!(Test-Path -LiteralPath $destination)){
    Copy-Item -LiteralPath $preview -Destination $destination
}
if((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $hash){throw 'Published copy hash mismatch.'}
if($CampaignWorker){
    $workerDestination=Join-Path (Join-Path $repo 'src\target\debug') 'concat-campaign-worker.exe'
    $workerHash=(Get-FileHash -LiteralPath $workerSource -Algorithm SHA256).Hash.ToLowerInvariant()
    if((Get-FileHash -LiteralPath $workerDestination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $workerHash){throw 'Published worker hash mismatch.'}
}
$receipt=[ordered]@{
    version=1;executable=$name;sha256=$hash;uiRevision=$UiRevision
    verifiedAtUtc=[DateTime]::UtcNow.ToString('o')
    uiSourceSha256=(Get-FileHash -LiteralPath (Join-Path $repo 'src\crates\concat\ui\workspace\studio-ai-control.slint') -Algorithm SHA256).Hash
}
if($CampaignWorker){$receipt.campaignWorker='concat-campaign-worker.exe';$receipt.campaignWorkerSha256=$workerHash}
$ready=Join-Path $repo '.runtime\kedi-current-build.json'
if(!$CampaignWorker -and (Test-Path -LiteralPath $ready)){
    # A view-only publication must retain the independently verified worker.
    $previousReceipt=Get-Content -LiteralPath $ready -Raw | ConvertFrom-Json
    if($previousReceipt.campaignWorkerSha256){
        if($previousReceipt.campaignWorker -ne 'concat-campaign-worker.exe' -or
           (Get-FileHash -LiteralPath $workerSource -Algorithm SHA256).Hash.ToLowerInvariant() -ne $previousReceipt.campaignWorkerSha256.ToLowerInvariant()){
            throw 'Existing campaign worker no longer matches its receipt. Nothing was published.'
        }
        foreach($field in @('campaignWorker','campaignWorkerSha256','campaignWorkerRevision','workerVerifiedAtUtc')){
            if($previousReceipt.PSObject.Properties.Name -contains $field){$receipt[$field]=$previousReceipt.$field}
        }
    }
}
$staged="$ready.new"
$json=$receipt | ConvertTo-Json
[IO.File]::WriteAllText($staged,$json,[Text.UTF8Encoding]::new($false))
if(Test-Path -LiteralPath $ready){[IO.File]::Replace($staged,$ready,"$ready.previous")}else{[IO.File]::Move($staged,$ready)}
Write-Output "PUBLISHED_EXECUTABLE=$destination"
Write-Output "SHA256=$hash"
Write-Output 'run-concat.bat and windows-studio.ps1 -RunBuilt now select this verified build. -PreviousBuild keeps the original rollback path.'
if($CampaignWorker){Write-Output "CAMPAIGN_WORKER=$workerDestination";Write-Output "CAMPAIGN_WORKER_SHA256=$workerHash"}
