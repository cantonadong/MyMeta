# Builds the web UI, embeds it into the release exe and copies MyMeta.exe to the project root.
$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot
Push-Location web
try {
    if (-not (Test-Path node_modules)) { npm install --no-audit --no-fund; if ($LASTEXITCODE -ne 0) { throw 'npm install failed.' } }
    npm run build
    if ($LASTEXITCODE -ne 0) { throw 'Web build failed.' }
} finally { Pop-Location }
cargo build --release --locked
if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
Get-Process -Name MyMeta -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 300
Copy-Item -LiteralPath 'target/release/MyMeta.exe' -Destination 'MyMeta.exe' -Force
Get-Item -LiteralPath 'MyMeta.exe' | Select-Object FullName, Length, LastWriteTime
