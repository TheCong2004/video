function Resolve-KediBuild {
    param([Parameter(Mandatory=$true)][string]$Repo,[switch]$UseExistingProfile,[switch]$Preview,[switch]$PreviousBuild,[string]$CandidateName)
    if($CandidateName -and (!$Preview -or $CandidateName -notmatch '^concat-ai-control-[a-z0-9-]+\.exe$')){throw 'CandidateName requires Preview and a local AI control executable name.'}
    if($Preview -and $UseExistingProfile){throw 'Preview uses its isolated portable profile. Omit -UseExistingProfile.'}
    $folder=if($UseExistingProfile){Join-Path $Repo 'src\target\debug'}else{Join-Path $Repo 'dist\kedi-test'}
    $exe=Join-Path $folder $(if($Preview){'concat-ai-control-preview.exe'}else{'concat.exe'})
    if($CandidateName){$exe=Join-Path $folder $CandidateName}
    $revision=$null
    $ready=Join-Path $Repo '.runtime\kedi-current-build.json'
    if($PreviousBuild){$ready="$ready.previous"}
    if($UseExistingProfile -and (Test-Path -LiteralPath $ready -PathType Leaf)){
        if((Get-Item -LiteralPath $ready).Length -gt 16384){throw 'Verified-build receipt exceeds 16 KiB.'}
        try {$receipt=Get-Content -LiteralPath $ready -Raw | ConvertFrom-Json}
        catch {throw 'Invalid verified-build receipt JSON.'}
        if($receipt.version -ne 1 -or $receipt.executable -notmatch '^concat-ai-control-[a-f0-9]{12}\.exe$' -or $receipt.sha256 -notmatch '^[a-fA-F0-9]{64}$'){
            throw 'Invalid verified-build receipt. Use -PreviousBuild for the original executable.'
        }
        $exe=Join-Path $folder $receipt.executable
        if(!(Test-Path -LiteralPath $exe -PathType Leaf) -or (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ne $receipt.sha256){
            throw 'Verified executable is missing or changed. Use -PreviousBuild or rebuild.'
        }
        if($receipt.campaignWorker){
            if($receipt.campaignWorker -ne 'concat-campaign-worker.exe' -or $receipt.campaignWorkerSha256 -notmatch '^[a-fA-F0-9]{64}$'){throw 'Invalid campaign worker receipt.'}
            $worker=Join-Path $folder $receipt.campaignWorker
            if(!(Test-Path -LiteralPath $worker -PathType Leaf) -or (Get-FileHash -LiteralPath $worker -Algorithm SHA256).Hash -ne $receipt.campaignWorkerSha256){throw 'Campaign worker is missing or changed. Rebuild the verified application.'}
        }
        $revision=[string]$receipt.uiRevision
    }
    if(!(Test-Path -LiteralPath $exe -PathType Leaf)){throw "Built executable missing: $exe"}
    [pscustomobject]@{Executable=$exe;Folder=$folder;VerifiedRevision=$revision}
}
