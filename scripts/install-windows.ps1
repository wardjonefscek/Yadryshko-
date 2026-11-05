# Сборка Cad+ и ярлык на рабочем столе (Windows 10/11).
# Запуск из папки репозитория:
#   powershell -ExecutionPolicy Bypass -File scripts\install-windows.ps1
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host "Сборка Cad+ (первый раз 5-10 минут)..."
cargo build --release -p cadplus-gui
if ($LASTEXITCODE -ne 0) { throw "Сборка не удалась" }

$dest = Join-Path $env:LOCALAPPDATA "Cad+"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item (Join-Path $root "target\release\cadplus.exe") (Join-Path $dest "cadplus.exe") -Force

$desktop = [Environment]::GetFolderPath("Desktop")
$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut((Join-Path $desktop "Cad+.lnk"))
$lnk.TargetPath = Join-Path $dest "cadplus.exe"
$lnk.WorkingDirectory = $dest
$lnk.Description = "Cad+ - крыло, лонжероны и нервюры"
$lnk.Save()
Write-Host "Готово: ярлык Cad+ на рабочем столе. Программа: $dest\cadplus.exe"
