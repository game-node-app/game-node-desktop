#!/usr/bin/env bash
# Build and run a separate Windows Tauri development executable using WSL's Vite server.
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

for tool in cargo cargo-xwin clang-cl llvm-lib lld-link llvm-rc cmake ninja pnpm node python3 curl powershell.exe wslpath flock; do
    command -v "$tool" >/dev/null || { echo "Missing required tool: $tool" >&2; exit 1; }
done

# A second copy would forward activation to the first app instead of opening the dev build.
check_app_stopped() {
    if ! powershell.exe -NoProfile -NonInteractive -Command \
        'if (Get-Process -Name gamenode -ErrorAction SilentlyContinue) { exit 1 }' >/dev/null; then
        echo 'GameNode is already running in Windows. Quit it from the app menu or system tray, then retry.' >&2
        exit 1
    fi
}
check_app_stopped

# Serialize launchers without interfering with the installed application or normal Cargo target.
mkdir -p target/windows-hmr
exec 9>target/windows-hmr/.launcher.lock
if ! flock -n 9; then
    echo 'Another Windows development launcher is already running.' >&2
    exit 1
fi

if [[ ! -f ui/node_modules/vite/bin/vite.js ]]; then
    echo 'Installing locked frontend dependencies...'
    pnpm --dir ui install --frozen-lockfile
fi

# Fail before a long build if Vite cannot own its fixed port.
if ! python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 4777)); s.close()' 2>/dev/null; then
    echo 'Port 4777 is in use in WSL. Stop the existing server before starting this launcher.' >&2
    exit 1
fi

target_dir="$repo_root/target/windows-hmr"
binary_dir="$target_dir/x86_64-pc-windows-msvc/debug"
echo 'Building the current Windows development executable...'
XWIN_ARCH=x86_64 CARGO_TARGET_DIR="$target_dir" cargo xwin build \
    --locked -p gamenode-app --no-default-features --target x86_64-pc-windows-msvc

# The native host expects its pinned backup helper and manifest beside the executable.
if [[ ! -d "$binary_dir/backup-assets" && -d dist/windows/gamenode-desktop/backup-assets ]]; then
    cp -a dist/windows/gamenode-desktop/backup-assets "$binary_dir/backup-assets"
fi
python3 scripts/prepare-backup-assets.py "$binary_dir/backup-assets"

# The installed app might have started while the build was running.
check_app_stopped

vite_log="$(mktemp)"
vite_pid=''
cleanup() {
    if [[ -n "$vite_pid" ]]; then
        kill "$vite_pid" 2>/dev/null || true
        wait "$vite_pid" 2>/dev/null || true
    fi
    rm -f "$vite_log"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Force the real bridge even if the caller exported VITE_UI_MOCK or an .env file enables it.
(
    cd ui
    exec env VITE_UI_MOCK=0 node node_modules/vite/bin/vite.js --host 127.0.0.1 --port 4777 --strictPort
) >"$vite_log" 2>&1 &
vite_pid=$!

ready=false
for ((attempt = 0; attempt < 100; attempt++)); do
    if ! kill -0 "$vite_pid" 2>/dev/null; then
        echo 'Vite could not start:' >&2
        cat "$vite_log" >&2
        exit 1
    fi
    if curl --noproxy '*' -fsS --max-time 1 http://127.0.0.1:4777/@vite/client >/dev/null; then
        ready=true
        break
    fi
    sleep 0.1
done
if [[ "$ready" != true ]]; then
    echo 'Timed out waiting for Vite:' >&2
    cat "$vite_log" >&2
    exit 1
fi

# Verify the same address is reachable from Windows before starting the webview.
if ! powershell.exe -NoProfile -NonInteractive -Command \
    'try { $r = Invoke-WebRequest -Uri "http://localhost:4777/@vite/client" -UseBasicParsing -TimeoutSec 5; if ($r.StatusCode -ne 200) { exit 1 } } catch { exit 1 }' >/dev/null; then
    echo 'Windows cannot reach Vite at http://localhost:4777. Check WSL localhost forwarding and Windows firewall settings.' >&2
    exit 1
fi
check_app_stopped

echo 'Starting the Windows development app. UI changes reload automatically.'
echo 'Quit GameNode from its menu or tray to stop the app and Vite. Rust changes require restarting this script.'
windows_exe="$(wslpath -w "$binary_dir/gamenode.exe")"
windows_launcher="$(wslpath -w "$repo_root/scripts/dev-windows-wsl.ps1")"
powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$windows_launcher" -ExecutablePath "$windows_exe"
