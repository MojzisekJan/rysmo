# Run in a Developer PowerShell with the Visual Studio C++ tools and Windows SDK.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
cargo build --locked -p rysmo_app --bin rysmo --target x86_64-pc-windows-msvc --profile portable
if ($LASTEXITCODE -ne 0) { throw "Windows build failed" }
$dest = "target/packages/windows"
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item "target/x86_64-pc-windows-msvc/portable/rysmo.exe" $dest
Copy-Item LICENSE, NOTICE.md $dest
Copy-Item "docs/licenses/phosphor-icons-MIT.txt" $dest
Copy-Item "crates/rysmo_app/brand/favicon.ico" "$dest/rysmo.ico"
Write-Host "Built $dest/rysmo.exe. Runtime checks and release packaging remain separate."
