$exe = "d:\capcutpolot\concat\src\target\debug\concat.exe"
if (!(Test-Path $exe)) { throw "concat.exe not found" }
$hash = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash
$p = $hash.Substring(0, 12).ToLower()
$name = "concat-ai-control-$p.exe"
$dest = "d:\capcutpolot\concat\src\target\debug\$name"
Copy-Item -LiteralPath $exe -Destination $dest -Force
$receipt = [ordered]@{
    version = 1
    executable = $name
    sha256 = $hash
    uiRevision = "Chanh Dien Online Live Studio 2.5D / 2026-11-02"
}
$receipt | ConvertTo-Json | Set-Content "d:\capcutpolot\concat\.runtime\kedi-current-build.json"
Write-Output "Successfully registered $name in kedi-current-build.json"
