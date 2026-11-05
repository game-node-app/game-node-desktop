#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
for tool in cargo clang-cl llvm-lib lld-link llvm-rc cmake ninja; do
    command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
export XWIN_ARCH=x86_64
cargo xwin build --workspace --target x86_64-pc-windows-msvc --locked
package=dist/windows/gamenode-desktop
mkdir -p "$package"
cp target/x86_64-pc-windows-msvc/debug/gamenode.exe "$package/"
cp LICENSE packaging/windows/README.md packaging/windows/*.ps1 "$package/"
cp -R licenses "$package/"
python3 scripts/prepare-backup-assets.py "$package/backup-assets"
echo "Package: $PWD/$package"
if command -v powershell.exe >/dev/null && command -v wslpath >/dev/null; then
    echo "Install: powershell.exe -NoProfile -ExecutionPolicy Bypass -File $(wslpath -w "$PWD/$package/install.ps1")"
else
    echo "Install: run install.ps1 from Windows PowerShell after copying the package to Windows."
fi
