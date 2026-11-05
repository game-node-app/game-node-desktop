#!/usr/bin/env bash
set -euo pipefail

integration_force=false
case "${1:-}" in
    --force) integration_force=true; shift ;;
    --help)
        echo 'Usage: ./install.sh [--force]'
        echo '  --force  Explicitly replace the current per-user protocol association.'
        exit 0 ;;
esac
[[ $# == 0 ]] || { echo 'Usage: ./install.sh [--force]' >&2; exit 1; }
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
install_dir=${GAMENODE_INSTALL_DIR:-"$HOME/.local/opt/gamenode-desktop"}
[[ "$install_dir" == /* && ! -L "$install_dir" ]] || {
    echo 'Use an absolute, non-symlink installation directory.' >&2
    exit 1
}
files=(gamenode gamenode.sh desktop-integration.sh VERSION build-manifest.json LICENSE README.md licenses gamenode.png install.sh uninstall.sh)
for name in "${files[@]}"; do
    [[ -e "$source_dir/$name" ]] || { echo "Package is missing $name" >&2; exit 1; }
done
if [[ -e "$install_dir" ]]; then
    marker="$install_dir/.gamenode-desktop-install"
    [[ ! -L "$marker" && -f "$marker" ]] &&
        grep -Fxq 'product=app.gamenode.desktop' "$marker" || {
            echo "Refusing to adopt unowned directory: $install_dir" >&2
            exit 1
        }
fi
integration_dir=$(realpath -m -- "$install_dir")
integration_mode=installed
source "$source_dir/desktop-integration.sh"
integration_prepare
mkdir -p "$integration_dir"
if [[ "$source_dir" != "$integration_dir" ]]; then
    for name in "${files[@]}"; do
        [[ ! -L "$integration_dir/$name" ]] || {
            echo "Refusing symlink installation file: $integration_dir/$name" >&2
            exit 1
        }
    done
    for name in "${files[@]}"; do
        cp -R -- "$source_dir/$name" "$integration_dir/"
    done
fi
chmod 755 "$integration_dir/gamenode" "$integration_dir/gamenode.sh" \
    "$integration_dir/install.sh" "$integration_dir/uninstall.sh"
integration_register
printf 'Installed: %s\n' "$integration_dir/gamenode.sh"
