#!/usr/bin/env bash
# Build the desktop and its bundled Steam provider into one target directory, then run the host.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

case "$(uname -s)" in
    Linux) desktop_name=gamenode ; plugin_name=gamenode-plugin-steam ;;
    MINGW*|MSYS*|CYGWIN*) desktop_name=gamenode.exe ; plugin_name=gamenode-plugin-steam.exe ;;
    *) echo 'Native desktop development supports Linux and Windows.' >&2; exit 1 ;;
esac

# Cargo options precede `--`; arguments after it are passed to GameNode.
cargo_args=()
app_args=()
while (($#)); do
    if [[ "$1" == -- ]]; then
        shift
        app_args=("$@")
        break
    fi
    case "$1" in
        --target|--target=*)
            echo 'run-desktop.sh builds for the current host; use the platform build script for cross-builds.' >&2
            exit 2
            ;;
    esac
    cargo_args+=("$1")
    shift
done

cargo build --locked --bins -p gamenode-app -p gamenode-plugin-steam "${cargo_args[@]}"
target_dir=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
profile=debug
for arg in "${cargo_args[@]}"; do
    if [[ "$arg" == --release ]]; then
        profile=release
    fi
done
binary_dir="$target_dir/$profile"
desktop="$binary_dir/$desktop_name"
plugin="$binary_dir/$plugin_name"
if [[ ! -f "$desktop" ]]; then
    echo "Desktop executable was not built: $desktop" >&2
    exit 1
fi
if [[ ! -f "$plugin" ]]; then
    echo "Steam plugin was not built beside the desktop: $plugin" >&2
    exit 1
fi

exec "$desktop" "${app_args[@]}"
