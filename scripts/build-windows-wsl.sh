#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
for tool in cargo cargo-xwin clang-cl llvm-lib lld-link llvm-rc cmake ninja pnpm python3; do
    command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done

plugin_cargo_output=$(python3 scripts/bundled_plugins.py --platform windows --format cargo-args)
plugin_files_output=$(python3 scripts/bundled_plugins.py --platform windows --format files)
plugin_cargo_args=()
while IFS= read -r plugin_arg; do
    [[ -n "$plugin_arg" ]] && plugin_cargo_args+=("$plugin_arg")
done <<< "$plugin_cargo_output"
plugin_files=()
while IFS= read -r plugin_file; do
    [[ -n "$plugin_file" ]] && plugin_files+=("$plugin_file")
done <<< "$plugin_files_output"
if [[ ${#plugin_files[@]} -eq 0 ]]; then
    echo 'No bundled plugins are declared for Windows.' >&2
    exit 1
fi

pnpm --dir ui build
export XWIN_ARCH=x86_64
cargo xwin build --bins -p gamenode-app --release --target x86_64-pc-windows-msvc --locked
cargo xwin build "${plugin_cargo_args[@]}" --release --target x86_64-pc-windows-msvc --locked
package=dist/windows/gamenode-desktop
mkdir -p "$package"
target_dir=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
binary_dir="$target_dir/x86_64-pc-windows-msvc/release"
binaries=("$binary_dir/gamenode.exe")
for plugin_file in "${plugin_files[@]}"; do
    [[ -f "$binary_dir/$plugin_file" ]] || {
        echo "Bundled plugin was not built: $binary_dir/$plugin_file" >&2
        exit 1
    }
    binaries+=("$binary_dir/$plugin_file")
done
cp "${binaries[@]}" "$package/"
cp LICENSE packaging/windows/README.md packaging/windows/*.ps1 "$package/"
cp -R licenses "$package/"
python3 - "$package" <<'PY'
from pathlib import Path
import sys

sys.path.insert(0, "scripts")
import bundled_plugins

bundled_plugins.write_index(Path(sys.argv[1]), "windows")
PY
python3 scripts/prepare-backup-assets.py "$package/backup-assets"
echo "Package: $PWD/$package"
if command -v powershell.exe >/dev/null && command -v wslpath >/dev/null; then
    echo "Install: powershell.exe -NoProfile -ExecutionPolicy Bypass -File $(wslpath -w "$PWD/$package/install.ps1")"
else
    echo "Install: run install.ps1 from Windows PowerShell after copying the package to Windows."
fi
