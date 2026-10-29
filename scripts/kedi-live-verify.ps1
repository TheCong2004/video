param([switch]$CleanBuild)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Split-Path -Parent $PSScriptRoot))
$source=Join-Path $repo 'src'
$logRoot=Join-Path $repo '.runtime\kedi-live-verification'
New-Item -ItemType Directory -Path $logRoot -Force | Out-Null
function Invoke-KediCargo([string[]]$Arguments,[string]$Log){
    $previousPreference=$ErrorActionPreference
    try{$ErrorActionPreference='Continue'; & cargo @Arguments *> $Log; $code=$LASTEXITCODE}
    finally{$ErrorActionPreference=$previousPreference}
    if($code -ne 0){Get-Content -LiteralPath $Log -Tail 50;throw "Cargo failed (exit $code): $($Arguments -join ' ')"}
}
Push-Location -LiteralPath $source
try {
    Invoke-KediCargo -Arguments @('test','-p','concat-studio','--offline','--','--test-threads=1') -Log (Join-Path $logRoot 'tests.log')
    Invoke-KediCargo -Arguments @('check','-p','concat-studio','--examples','--offline') -Log (Join-Path $logRoot 'examples.log')
    $ErrorActionPreference='Continue'
    $actions=& cargo run -p concat-studio --example kedi_live_replay --offline --quiet 2> (Join-Path $logRoot 'replay-stderr.log')
    $replayCode=$LASTEXITCODE
    $ErrorActionPreference='Stop'
    if($replayCode -ne 0){throw 'Kedi replay failed'}
    $actions|Set-Content -LiteralPath (Join-Path $logRoot 'actions.ndjson') -Encoding UTF8
    Get-Content (Join-Path $logRoot 'tests.log') | Select-String 'test result:'
    Write-Output "KEDI_LOCAL_VERIFICATION=PASS; logs=$logRoot"
} finally {
    Pop-Location
    if($CleanBuild){
        $target=Join-Path $source 'target'
        if(Test-Path -LiteralPath $target){
            $resolved=(Resolve-Path -LiteralPath $target).Path
            if($resolved -ne $target -or !(Test-Path -LiteralPath (Join-Path $resolved '.rustc_info.json'))){throw 'Refusing cleanup: Cargo target ownership could not be verified'}
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
}
