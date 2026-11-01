# Forward to root dev-concat.ps1
$concatRoot = Split-Path -Parent $PSScriptRoot
& "$concatRoot\dev-concat.ps1"

