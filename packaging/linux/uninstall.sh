#!/usr/bin/env bash
set -euo pipefail

install_dir=${GAMENODE_INSTALL_DIR:-"$HOME/.local/opt/gamenode-desktop"}
marker="$install_dir/.gamenode-desktop-install"
[[ -f "$marker" ]] || { echo 'GameNode Desktop installation marker not found' >&2; exit 1; }

product=''
previous_handler=''
data_home=''
config_home=''
while IFS='=' read -r key value; do
    case "$key" in
        product) product=$value ;;
        previous-handler) previous_handler=$value ;;
        data-home) data_home=$value ;;
        config-home) config_home=$value ;;
    esac
done < "$marker"
[[ "$product" == 'app.gamenode.desktop' ]] || { echo 'Installation is not owned by GameNode Desktop' >&2; exit 1; }
[[ -n "$data_home" && -n "$config_home" ]] || { echo 'Installation marker is incomplete' >&2; exit 1; }

desktop_file="$data_home/applications/gamenode-desktop.desktop"
scheme='x-scheme-handler/app.gamenode'
export XDG_DATA_HOME="$data_home"
export XDG_CONFIG_HOME="$config_home"
if command -v xdg-mime >/dev/null && [[ "$(xdg-mime query default "$scheme" 2>/dev/null || true)" == 'gamenode-desktop.desktop' ]]; then
    if [[ -n "$previous_handler" && "$previous_handler" != 'gamenode-desktop.desktop' ]]; then
        xdg-mime default "$previous_handler" "$scheme"
    else
        mimeapps="$config_home/mimeapps.list"
        if [[ -f "$mimeapps" ]]; then
            tmp="$mimeapps.tmp.$$"
            awk '
                /^\[Default Applications\]$/ { in_defaults=1; print; next }
                /^\[/ { in_defaults=0 }
                in_defaults && /^x-scheme-handler\/app\.gamenode=/ { next }
                { print }
            ' "$mimeapps" > "$tmp"
            mv -f "$tmp" "$mimeapps"
        fi
    fi
fi

if [[ -f "$desktop_file" ]] && grep -Fxq 'X-GameNode-Desktop-Owner=app.gamenode.desktop' "$desktop_file"; then
    rm -f "$desktop_file"
fi
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$data_home/applications" || true
fi

rm -f "$install_dir/gamenode" "$install_dir/gamenode.png" "$install_dir/LICENSE" "$install_dir/README.md"
rm -f "$install_dir/install.sh" "$install_dir/uninstall.sh" "$marker"
rm -rf "$install_dir/licenses"
rmdir "$install_dir" 2>/dev/null || true
printf 'Removed GameNode Desktop installation and owned desktop integration. User data was preserved.\n'
