# ==============================================================================
# CONCAT STUDIO + POSTIZ FAST REFRESH DEV RUNNER
# ==============================================================================
# Che do: Concat Rust + Slint Native UI + Postiz Next.js HMR (Fast Refresh)
# Ban chi can sua file .tsx / .css trong postiz-app va Luu -> Concat tu cap nhat!
# KHONG can cargo build lai khi sua frontend.
# ==============================================================================

param(
    [switch]$RebuildRust,
    [int]$Port = 3000
)

$ErrorActionPreference = 'Stop'
$concatRoot = $PSScriptRoot
$postizRoot = 'D:\capcutpolot\postiz-app'
$frontendRoot = Join-Path $postizRoot 'apps\frontend'

Write-Host ""
Write-Host "========================================================================" -ForegroundColor Cyan
Write-Host "   CONCAT STUDIO + POSTIZ FAST REFRESH DEV ENVIRONMENT" -ForegroundColor Green
Write-Host "========================================================================" -ForegroundColor Cyan
Write-Host " [MODE] CONCAT_POSTIZ_MODE = dev (Next.js HMR Live Dev)" -ForegroundColor Yellow
Write-Host " [HMR]  Sua file React/Next.js/CSS -> Luu -> Concat cap nhat tuc thi!" -ForegroundColor Yellow
Write-Host " [IPC]  JavaScript <-> Rust IPC Bridge hoat dong hai chieu" -ForegroundColor Yellow
Write-Host "========================================================================" -ForegroundColor Cyan
Write-Host ""

# 1. Kiem tra Cong cu: Node.js, pnpm, Rust
Write-Host "[1/4] Kiem tra moi truong phat trien..." -ForegroundColor Cyan
if (!(Get-Command node -ErrorAction SilentlyContinue)) {
    throw "Khong tim thay Node.js trong PATH. Vui long cai dat Node.js >= 20."
}
if (!(Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "Khong tim thay Cargo (Rust) trong PATH."
}
$nodeVer = node -v
Write-Host "  -> Node.js: $nodeVer" -ForegroundColor DarkGray
Write-Host "  -> Rust Cargo: san sang" -ForegroundColor DarkGray

# 2. Kiem tra & Khoi dong Next.js Dev Server tren loopback
Write-Host "[2/4] Kiem tra Next.js Dev Server (127.0.0.1:$Port)..." -ForegroundColor Cyan
$serverUrl = "http://127.0.0.1:$Port/launches"

$devServerRunning = $false
try {
    $res = Invoke-WebRequest -Uri $serverUrl -UseBasicParsing -TimeoutSec 2 -ErrorAction SilentlyContinue
    if ($res.StatusCode -eq 200) {
        $devServerRunning = $true
    }
} catch {
    $devServerRunning = $false
}

$spawnedNodeProcess = $null
if ($devServerRunning) {
    Write-Host "  -> Next.js Dev Server dang chay san sang tren port $Port." -ForegroundColor Green
} else {
    Write-Host "  -> Dang khoi dong Next.js Dev Server tren 127.0.0.1:$Port..." -ForegroundColor Yellow
    
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = "cmd.exe"
    $pinfo.Arguments = "/c npx next dev -p $Port -H 127.0.0.1"
    $pinfo.WorkingDirectory = $frontendRoot
    $pinfo.UseShellExecute = $true
    $pinfo.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Minimized
    
    $spawnedNodeProcess = [System.Diagnostics.Process]::Start($pinfo)
    
    Write-Host "  -> Dang cho Next.js Turbopack bien dich trang /launches..." -NoNewline -ForegroundColor Cyan
    $attempts = 0
    $maxAttempts = 90
    while ($attempts -lt $maxAttempts) {
        Start-Sleep -Seconds 1
        $attempts++
        Write-Host "." -NoNewline -ForegroundColor Cyan
        try {
            $check = Invoke-WebRequest -Uri $serverUrl -UseBasicParsing -TimeoutSec 2 -ErrorAction SilentlyContinue
            if ($check.StatusCode -eq 200) {
                $devServerRunning = $true
                break
            }
        } catch {
            # Continue waiting
        }
    }
    Write-Host ""
    if (!$devServerRunning) {
        throw "Next.js dev server khoi dong qua thoi gian cho (90s). Vui long kiem tra log."
    }
    Write-Host "  -> Next.js Dev Server da san sang tai $serverUrl!" -ForegroundColor Green
}

# 3. Thiet lap Bien moi truong DEV
Write-Host "[3/4] Cau hinh bien moi truong Concat Dev..." -ForegroundColor Cyan
$env:CONCAT_POSTIZ_MODE = "dev"
$env:POSTIZ_DEV_URL = $serverUrl
Write-Host "  -> CONCAT_POSTIZ_MODE = dev" -ForegroundColor Green
Write-Host "  -> POSTIZ_DEV_URL = $serverUrl" -ForegroundColor Green

# 4. Khoi dong Concat Studio
Write-Host "[4/4] Khoi chay Concat Studio..." -ForegroundColor Cyan

# Don dep tien trinh ngam hoac process cu khong co cua so
$staleProcesses = Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.ProcessName -like 'concat*' -and $_.ProcessName -ne 'concat-campaign-worker' }
foreach ($proc in $staleProcesses) {
    if (!$proc.MainWindowHandle -or $proc.MainWindowHandle -eq 0) {
        Write-Host "  -> Don dep tien trinh chay ngam cu (PID $($proc.Id))..." -ForegroundColor DarkGray
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
    }
}

$builtExe = Get-ChildItem (Join-Path $concatRoot 'src\target\debug') -Filter 'concat-ai-control-*.exe' -ErrorAction SilentlyContinue | Select-Object -Last 1

try {
    if ($RebuildRust -or !$builtExe) {
        Write-Host "  -> Thuc hien build Rust incremental mot lan duy nhat..." -ForegroundColor Yellow
        powershell -ExecutionPolicy Bypass -File "$concatRoot\scripts\windows-studio.ps1" -Build
    }
    
    Write-Host "========================================================================" -ForegroundColor Green
    Write-Host " [CONCAT DEV ACTIVE] App dang chay! Ban co the mo Edge DevTools bang F12" -ForegroundColor Green
    Write-Host " [FAST REFRESH] SANG POSTIZ SUA CODE REACT VA XEM NGAY TRUC TIEP TREN CONCAT" -ForegroundColor Yellow
    Write-Host "========================================================================" -ForegroundColor Green
    Write-Host ""
    
    powershell -ExecutionPolicy Bypass -File "$concatRoot\scripts\windows-studio.ps1" -RunBuilt
} finally {
    Write-Host ""
    Write-Host "[*] Concat Studio da dong." -ForegroundColor Cyan
}

