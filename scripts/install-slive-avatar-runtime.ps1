param([switch]$CheckOnly)
$ErrorActionPreference = 'Stop'
$concatRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$runtimeRoot = Join-Path $concatRoot '.runtime'
$pythonRoot = Join-Path $runtimeRoot 'python'
$venvRoot = Join-Path $runtimeRoot 'slive-venv'
$modelRoot = Join-Path $runtimeRoot 'slive-models'
$modelPath = Join-Path $modelRoot 's3fd.pth'
$pythonExe = Join-Path $venvRoot 'Scripts/python.exe'
$adapter = Join-Path $concatRoot 'integrations/slive/runtime/avatar_prepare.py'
$expectedHash = '619A31681264D3F7F7FC7A16A42CBBE8B23F31A256F75A366E5A1BCD59B33543'
$names = @('UV_CACHE_DIR', 'UV_PYTHON_INSTALL_DIR', 'TEMP', 'TMP')
$saved = @{}
foreach ($name in $names) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    New-Item -ItemType Directory -Path $runtimeRoot, $modelRoot, (Join-Path $runtimeRoot 'test-temp') -Force | Out-Null
    $env:UV_CACHE_DIR = Join-Path $runtimeRoot 'uv-cache'
    $env:UV_PYTHON_INSTALL_DIR = $pythonRoot
    $env:TEMP = Join-Path $runtimeRoot 'test-temp'
    $env:TMP = $env:TEMP
    if (!$CheckOnly) {
        Get-Command uv -ErrorAction Stop | Out-Null
        & uv python install 3.11.15 --no-bin --no-registry --install-dir $pythonRoot
        if ($LASTEXITCODE -ne 0) { throw 'Python installation failed.' }
        $managedPython = Join-Path $pythonRoot 'cpython-3.11.15-windows-x86_64-none/python.exe'
        if (!(Test-Path -LiteralPath $pythonExe)) {
            & uv venv --python $managedPython $venvRoot
            if ($LASTEXITCODE -ne 0) { throw 'Virtual environment creation failed.' }
        }
        & uv pip install --python $pythonExe --no-cache 'torch==2.14.1+cpu' --index-url 'https://download.pytorch.org/whl/cpu'
        if ($LASTEXITCODE -ne 0) { throw 'Torch installation failed.' }
        $requirements = @(Get-Content -LiteralPath (Join-Path $concatRoot 'integrations/slive/runtime/requirements-avatar.lock.txt') | Where-Object { $_ -and !($_.StartsWith('#')) -and !($_.StartsWith('torch==')) })
        & uv pip install --python $pythonExe --no-cache --default-index 'https://pypi.org/simple' @requirements
        if ($LASTEXITCODE -ne 0) { throw 'Avatar dependency installation failed.' }
        if (!(Test-Path -LiteralPath $modelPath)) {
            $download = Join-Path $modelRoot 's3fd.pth.download'
            if (Test-Path -LiteralPath $download) { throw 'A previous partial model download exists; inspect it before retrying.' }
            & curl.exe --fail --location --max-time 180 --proto '=https' --output $download 'https://www.adrianbulat.com/downloads/python-fan/s3fd-619a316812.pth'
            if ($LASTEXITCODE -ne 0) { throw 'S3FD download failed.' }
            if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash -ne $expectedHash) { throw 'S3FD checksum mismatch; file was not activated.' }
            Move-Item -LiteralPath $download -Destination $modelPath
        }
    }
    if (!(Test-Path -LiteralPath $pythonExe)) { throw 'Run the installer first; Concat Python is missing.' }
    if (!(Test-Path -LiteralPath $modelPath) -or (Get-FileHash -LiteralPath $modelPath -Algorithm SHA256).Hash -ne $expectedHash) { throw 'Concat S3FD checkpoint is missing or invalid.' }
    $check = & $pythonExe -B $adapter --check
    if ($LASTEXITCODE -ne 0) { throw 'Runtime check failed.' }
    Write-Output $check
    if (!(($check | ConvertFrom-Json).ready)) { throw 'Runtime dependencies are not ready.' }
    Write-Host 'Concat runtime checked. No ArtCraft paths or system Python settings were changed.'
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
