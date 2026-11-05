#!/usr/bin/env bash
set -euo pipefail

install_dir=${GAMENODE_INSTALL_DIR:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)}
marker="$install_dir/.gamenode-desktop-install"
[[ ! -L "$marker" && -f "$marker" ]] || { echo 'GameNode Desktop installation marker not found or unsafe' >&2; exit 1; }

product=''
data_home=''
config_home=''
mode=''
previous_handler=''
while IFS='=' read -r key value; do
    case "$key" in
        product) product=$value ;;
        data-home) data_home=$value ;;
        config-home) config_home=$value ;;
        mode) mode=$value ;;
        previous-handler) previous_handler=$value ;;
    esac
done < "$marker"
[[ "$product" == 'app.gamenode.desktop' ]] || { echo 'Installation is not owned by GameNode Desktop' >&2; exit 1; }
[[ "$data_home" == /* && "$config_home" == /* ]] || { echo 'Installation marker is incomplete' >&2; exit 1; }
[[ "$mode" == installed || "$mode" == portable ]] || { echo 'Unknown installation mode; use the original uninstaller' >&2; exit 1; }
if [[ "$mode" == installed && -L "$install_dir/libexec" ]]; then
    echo 'Refusing symlink libexec directory' >&2
    exit 1
fi

desktop_file="$data_home/applications/gamenode-desktop.desktop"
if [[ -L "$desktop_file" ]]; then
    echo 'Refusing symlink desktop entry' >&2
    exit 1
fi
if [[ -f "$desktop_file" ]] &&
   grep -Fxq 'X-GameNode-Desktop-Owner=app.gamenode.desktop' "$desktop_file" &&
   grep -Fxq "X-GameNode-Desktop-Directory=$install_dir" "$desktop_file"; then
    # Remove only our desktop ID, including desktop-specific KDE defaults.
    shopt -s nullglob
    mime_files=("$config_home/mimeapps.list" "$config_home/"*-mimeapps.list
        "$data_home/applications/mimeapps.list" "$data_home/applications/"*-mimeapps.list)
    for mimeapps in "${mime_files[@]}"; do
        [[ ! -L "$mimeapps" ]] || { echo "Refusing symlink MIME settings: $mimeapps" >&2; exit 1; }
    done
    export XDG_DATA_HOME="$data_home" XDG_CONFIG_HOME="$config_home"
    scheme='x-scheme-handler/app.gamenode'
    source "$install_dir/desktop-integration.sh"
    if [[ -n "$previous_handler" && "$previous_handler" != gamenode-desktop.desktop ]] &&
       integration_handler_exists "$previous_handler"; then
        command -v xdg-mime >/dev/null || { echo 'Install xdg-utils to restore the previous handler before uninstalling.' >&2; exit 1; }
        current_handler=$(xdg-mime query default "$scheme")
        if [[ "$current_handler" == gamenode-desktop.desktop ]]; then
            integration_set_default "$previous_handler" || {
                echo 'Could not restore the previous association; integration was retained. Fix the error and retry.' >&2
                exit 1
            }
        fi
    fi
    for mimeapps in "${mime_files[@]}"; do
        [[ -f "$mimeapps" ]] || continue
        tmp=$(mktemp "${mimeapps}.XXXXXX")
        awk '
            /^\[/ { associations=($0 == "[Default Applications]" || $0 == "[Added Associations]") }
            associations && /^x-scheme-handler\/app\.gamenode=/ {
                count=split(substr($0, index($0, "=")+1), ids, ";"); remaining=""
                for (i=1; i<=count; i++)
                    if (ids[i] != "" && ids[i] != "gamenode-desktop.desktop") remaining=remaining ids[i] ";"
                if (remaining != "") print "x-scheme-handler/app.gamenode=" remaining
                next
            }
            { print }
        ' "$mimeapps" > "$tmp"
        chmod --reference="$mimeapps" "$tmp"
        mv -f "$tmp" "$mimeapps"
    done
    rm -f "$desktop_file"
    if command -v update-desktop-database >/dev/null; then
        update-desktop-database "$data_home/applications" || true
    fi
fi

if [[ "$mode" == installed ]]; then
    rm -f "$install_dir/libexec/gamenode"
    if [[ -d "$install_dir/libexec" ]]; then
        rmdir "$install_dir/libexec" 2>/dev/null || true
    fi
    rm -f "$install_dir/gamenode" "$install_dir/gamenode.png" "$install_dir/LICENSE" "$install_dir/README.md"
    rm -f "$install_dir/install.sh" "$install_dir/uninstall.sh" "$install_dir/gamenode.sh" "$install_dir/desktop-integration.sh"
    rm -f "$install_dir/VERSION" "$install_dir/build-manifest.json"
    # Never recursively delete a directory that may now contain user files.
    echo 'Installed binaries removed. License files, ownership metadata and any other files were retained.'
else
    rm -f "$marker"
    echo 'Portable package retained; delete its extracted directory manually if no longer needed.'
fi
printf 'Removed matching desktop integration. User data was preserved.\n'
