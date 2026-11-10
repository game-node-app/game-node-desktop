#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
for tool in cargo cargo-xwin clang-cl llvm-lib lld-link llvm-rc cmake ninja pnpm python3; do
    command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
pnpm --dir ui build
export XWIN_ARCH=x86_64
cargo xwin build --bins -p gamenode-app -p gamenode-plugin-steam -p gamenode-plugin-epic \
    --release --target x86_64-pc-windows-msvc --locked
package=dist/windows/gamenode-desktop
mkdir -p "$package"
target_dir=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
binary_dir="$target_dir/x86_64-pc-windows-msvc/release"
cp "$binary_dir/gamenode.exe" "$binary_dir/gamenode-plugin-steam.exe" \
    "$binary_dir/gamenode-plugin-epic.exe" "$package/"
cp LICENSE packaging/windows/README.md packaging/windows/*.ps1 "$package/"
cp -R licenses "$package/"
python3 scripts/prepare-backup-assets.py "$package/backup-assets"
echo "Package: $PWD/$package"
if command -v powershell.exe >/dev/null && command -v wslpath >/dev/null; then
    echo "Install: powershell.exe -NoProfile -ExecutionPolicy Bypass -File $(wslpath -w "$PWD/$package/install.ps1")"
else
    echo "Install: run install.ps1 from Windows PowerShell after copying the package to Windows."
fi
