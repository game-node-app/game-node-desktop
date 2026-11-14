#!/usr/bin/env bash
# Build the desktop and its bundled provider plugins into one target directory, then run the host.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

# The plugin executable set comes from the trusted descriptors through scripts/bundled_plugins.py.
# Epic is Windows-only, so the Linux list naturally leaves it out.
case "$(uname -s)" in
    Linux) platform=linux; desktop_name=gamenode ;;
    MINGW*|MSYS*|CYGWIN*) platform=windows; desktop_name=gamenode.exe ;;
    *) echo 'Native desktop development supports Linux and Windows.' >&2; exit 1 ;;
esac

plugin_files_output=$(python3 scripts/bundled_plugins.py --platform "$platform" --format files)
plugin_cargo_output=$(python3 scripts/bundled_plugins.py --platform "$platform" --format cargo-args)
plugin_files=()
while IFS= read -r plugin_file; do
    [[ -n "$plugin_file" ]] && plugin_files+=("$plugin_file")
done <<< "$plugin_files_output"
plugin_cargo_args=()
while IFS= read -r plugin_arg; do
    [[ -n "$plugin_arg" ]] && plugin_cargo_args+=("$plugin_arg")
done <<< "$plugin_cargo_output"
if [[ ${#plugin_files[@]} -eq 0 ]]; then
    echo "No bundled plugins are declared for $platform." >&2
    exit 1
fi

# Cargo options precede `--`; arguments after it are passed to GameNode.
# Guard empty array expansions for nounset on Bash versions before 4.4.
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

cargo build --locked --bins -p gamenode-app ${cargo_args[@]+"${cargo_args[@]}"}
cargo build --locked "${plugin_cargo_args[@]}" ${cargo_args[@]+"${cargo_args[@]}"}
target_dir=$(cargo metadata --locked --no-deps --format-version 1 \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
profile=debug
for arg in ${cargo_args[@]+"${cargo_args[@]}"}; do
    if [[ "$arg" == --release ]]; then
        profile=release
    fi
done
binary_dir="$target_dir/$profile"
desktop="$binary_dir/$desktop_name"
if [[ ! -f "$desktop" ]]; then
    echo "Desktop executable was not built: $desktop" >&2
    exit 1
fi
for plugin_file in "${plugin_files[@]}"; do
    plugin="$binary_dir/$plugin_file"
    if [[ ! -f "$plugin" ]]; then
        echo "Required provider plugin was not built beside the desktop: $plugin" >&2
        exit 1
    fi
done

exec "$desktop" ${app_args[@]+"${app_args[@]}"}
