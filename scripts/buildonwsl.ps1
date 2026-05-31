$ErrorActionPreference = "Stop"

Write-Host "== Futureboard Linux Build via WSL Arch Linux =="

$Distro = "ArchLinux"

$LinuxScript = @'
set -euo pipefail

# แนะนำให้ repo อยู่ใน WSL filesystem จะเร็วกว่า /mnt/c มาก
# แก้ path นี้ให้ตรงเครื่องพี่
cd /mnt/h/ProjectsDev/Futureboard

echo "== Initializing submodules =="
bash .github/workflows/submodules-init.sh

echo "== Installing Rust target =="
rustup target add x86_64-unknown-linux-gnu

echo "== Building Futureboard native for Linux x64 =="
cargo build \
  --release \
  --workspace \
  --target x86_64-unknown-linux-gnu

echo "== Linux WSL Arch build done =="
'@

wsl -d $Distro -- bash -lc ($LinuxScript -replace "`r", "")
