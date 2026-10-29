$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'select-kedi-build.ps1')
$repo=Split-Path -Parent $PSScriptRoot
$testRoot=Join-Path (Join-Path $repo '.runtime') ('launcher-contract-'+[Guid]::NewGuid().ToString('N'))
$expected=[IO.Path]::GetFullPath($testRoot)
if(!$expected.StartsWith(([IO.Path]::GetFullPath((Join-Path $repo '.runtime'))+[IO.Path]::DirectorySeparatorChar),[StringComparison]::OrdinalIgnoreCase)){throw 'Unexpected test root'}
$passed=0
function Assert-True($value,[string]$message){if(!$value){throw $message};$script:passed++}
function Assert-Throws([scriptblock]$body){$threw=$false;try{& $body | Out-Null}catch{$threw=$true};Assert-True $threw 'Expected refusal'}
try {
    $native=Join-Path $testRoot 'src\target\debug'
    $portable=Join-Path $testRoot 'dist\kedi-test'
    New-Item -ItemType Directory -Force -Path $native,$portable,(Join-Path $testRoot '.runtime') | Out-Null
    # These are inert file-selection fixtures; no fixture is ever executed.
    $original=Join-Path $native 'concat.exe'
    [IO.File]::WriteAllText($original,'original fixture')
    [IO.File]::WriteAllText((Join-Path $portable 'concat.exe'),'portable fixture')
    Assert-True ((Resolve-KediBuild -Repo $testRoot -UseExistingProfile).Executable -eq $original) 'Original default changed'
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -UseExistingProfile -Preview}
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -Preview}
    $dual=Join-Path $portable 'concat-ai-control-dual.exe'
    [IO.File]::WriteAllText($dual,'dual-window candidate fixture')
    Assert-True ((Resolve-KediBuild -Repo $testRoot -Preview -CandidateName 'concat-ai-control-dual.exe').Executable -eq $dual) 'Wrong preview candidate selected'
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -CandidateName 'concat-ai-control-dual.exe'}
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -Preview -CandidateName '..\concat-ai-control-dual.exe'}
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -Preview -UseExistingProfile -CandidateName 'concat-ai-control-dual.exe'}
    $candidate=Join-Path $native 'concat-ai-control-0123456789ab.exe'
    [IO.File]::WriteAllText($candidate,'verified fixture')
    $hash=(Get-FileHash -LiteralPath $candidate).Hash
    $receipt=@{version=1;executable=[IO.Path]::GetFileName($candidate);sha256=$hash;uiRevision='test only'}
    $ready=Join-Path $testRoot '.runtime\kedi-current-build.json'
    $receipt | ConvertTo-Json | Set-Content -LiteralPath $ready
    Assert-True ((Resolve-KediBuild -Repo $testRoot -UseExistingProfile).Executable -eq $candidate) 'Verified build was not selected'
    Assert-True ((Resolve-KediBuild -Repo $testRoot -UseExistingProfile -PreviousBuild).Executable -eq $original) 'Rollback did not select original'
    [IO.File]::WriteAllText($candidate,'tampered fixture')
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -UseExistingProfile}
    $receipt.executable='..\outside.exe'
    $receipt | ConvertTo-Json | Set-Content -LiteralPath $ready
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -UseExistingProfile}
    Assert-True ((Resolve-KediBuild -Repo $testRoot -UseExistingProfile -PreviousBuild).Executable -eq $original) 'Invalid receipt blocked explicit rollback'
    [IO.File]::WriteAllText($ready,'not JSON')
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -UseExistingProfile}
    [IO.File]::WriteAllText($ready,('x'*16385))
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -UseExistingProfile}
    $rollback=Join-Path $native 'concat-ai-control-fedcba987654.exe'
    [IO.File]::WriteAllText($rollback,'prior verified fixture')
    @{version=1;executable=[IO.Path]::GetFileName($rollback);sha256=(Get-FileHash -LiteralPath $rollback).Hash;uiRevision='previous verified'} | ConvertTo-Json | Set-Content -LiteralPath "$ready.previous"
    Assert-True ((Resolve-KediBuild -Repo $testRoot -UseExistingProfile -PreviousBuild).Executable -eq $rollback) 'Rollback must use the previous verified receipt even after concat.exe is rebuilt'
    [IO.File]::WriteAllText($rollback,'tampered prior fixture')
    Assert-Throws {Resolve-KediBuild -Repo $testRoot -UseExistingProfile -PreviousBuild}
    "Launcher contract: $passed passed; no fixture process launched."
} finally {
    $resolved=(Resolve-Path -LiteralPath $testRoot).Path
    if($resolved -ne $expected){throw 'Unexpected cleanup target'}
    Remove-Item -LiteralPath $resolved -Recurse -Force
}
