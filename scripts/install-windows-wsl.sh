#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

for tool in powershell.exe wslpath; do
    command -v "$tool" >/dev/null || {
        echo "Missing WSL/Windows tool: $tool (run this script from WSL with Windows interop enabled)" >&2
        exit 1
    }
done

scripts/build-windows-wsl.sh

package_dir="$(wslpath -w "$repo_root/dist/windows/gamenode-desktop")"
installer="$(wslpath -w "$repo_root/dist/windows/gamenode-desktop/install.ps1")"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$installer" -SourceDirectory "$package_dir"

powershell.exe -NoProfile -Command \
    '$exe = Join-Path $env:LOCALAPPDATA "Programs\GameNode Desktop\gamenode.exe"; if (!(Test-Path -LiteralPath $exe)) { throw "Installed GameNode executable not found: $exe" }; Start-Process -FilePath $exe'
