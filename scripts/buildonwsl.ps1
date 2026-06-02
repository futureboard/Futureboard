$ErrorActionPreference = "Stop"

Write-Host "== Futureboard Linux Build via WSL Arch Linux =="

$Distro = "Arch"

$LinuxScript = @'
set -euo pipefail

cd /mnt/d/private/Futureboard

echo "== Building Futureboard native for Linux x64 =="
cargo build \
  --release \
  --workspace \
  --target x86_64-unknown-linux-gnu

echo "== Linux WSL Arch build done =="
'@

wsl -d $Distro -- bash -lc ($LinuxScript -replace "`r", "")
