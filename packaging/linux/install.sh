#!/usr/bin/env bash
set -euo pipefail

source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
install_dir=${GAMENODE_INSTALL_DIR:-"$HOME/.local/opt/gamenode-desktop"}
data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
marker="$install_dir/.gamenode-desktop-install"
desktop_file="$data_home/applications/gamenode-desktop.desktop"
scheme='x-scheme-handler/app.gamenode'
export XDG_DATA_HOME="$data_home"
export XDG_CONFIG_HOME="$config_home"

for name in gamenode LICENSE README.md licenses gamenode.png install.sh uninstall.sh; do
    [[ -e "$source_dir/$name" ]] || { echo "Package is missing $name" >&2; exit 1; }
done
command -v xdg-mime >/dev/null || { echo 'Install xdg-utils before registering the GameNode URI scheme.' >&2; exit 1; }

previous_handler=''
if [[ -L "$install_dir" ]]; then
    echo "Refusing symlink installation directory: $install_dir" >&2
    exit 1
fi
if [[ -e "$install_dir" ]]; then
    [[ -f "$marker" ]] || { echo "Refusing to adopt unowned directory: $install_dir" >&2; exit 1; }
    [[ ! -L "$marker" ]] || { echo 'Refusing symlink installation marker' >&2; exit 1; }
    product=''
    while IFS='=' read -r key value; do
        case "$key" in
            product) product=$value ;;
            previous-handler) previous_handler=$value ;;
        esac
    done < "$marker"
    [[ "$product" == 'app.gamenode.desktop' ]] || { echo 'Installation marker is not owned by GameNode Desktop' >&2; exit 1; }
else
    previous_handler=$(xdg-mime query default "$scheme" 2>/dev/null || true)
    [[ "$previous_handler" != *$'\n'* ]] || previous_handler=''
fi

if [[ -L "$desktop_file" ]]; then
    echo "Refusing symlink desktop entry: $desktop_file" >&2
    exit 1
fi
if [[ -e "$desktop_file" ]] && ! grep -Fxq 'X-GameNode-Desktop-Owner=app.gamenode.desktop' "$desktop_file"; then
    echo "Refusing to overwrite an unowned desktop entry: $desktop_file" >&2
    exit 1
fi

mkdir -p "$install_dir" "$data_home/applications"
source_dir=$(cd -- "$source_dir" && pwd -P)
install_dir=$(cd -- "$install_dir" && pwd -P)
marker="$install_dir/.gamenode-desktop-install"
if [[ "$source_dir" != "$install_dir" ]]; then
    cp -f "$source_dir/gamenode" "$install_dir/gamenode"
    cp -f "$source_dir/LICENSE" "$install_dir/LICENSE"
    cp -f "$source_dir/README.md" "$install_dir/README.md"
    cp -f "$source_dir/gamenode.png" "$install_dir/gamenode.png"
    cp -f "$source_dir/install.sh" "$install_dir/install.sh"
    cp -f "$source_dir/uninstall.sh" "$install_dir/uninstall.sh"
    mkdir -p "$install_dir/licenses"
    cp -R "$source_dir/licenses/." "$install_dir/licenses/"
fi
chmod 755 "$install_dir/gamenode" "$install_dir/install.sh" "$install_dir/uninstall.sh"

marker_tmp=$(mktemp "$install_dir/.gamenode-desktop-install.XXXXXX")
printf 'product=app.gamenode.desktop\nprevious-handler=%s\ndata-home=%s\nconfig-home=%s\n' \
    "$previous_handler" "$data_home" "$config_home" > "$marker_tmp"
chmod 600 "$marker_tmp"
mv -f "$marker_tmp" "$marker"

desktop_exec=$(printf '%s' "$install_dir/gamenode" | sed -e 's/\\/\\\\/g' -e 's/ /\\ /g' -e 's/"/\\"/g' -e 's/%/%%/g')
cat > "$desktop_file" <<EOF
[Desktop Entry]
Type=Application
Name=GameNode Desktop
Comment=Secure GameNode account sign-in
Exec=$desktop_exec %u
Icon=$install_dir/gamenode.png
Terminal=false
Categories=Game;Utility;
MimeType=x-scheme-handler/app.gamenode;
StartupNotify=true
X-GameNode-Desktop-Owner=app.gamenode.desktop
EOF

xdg-mime default gamenode-desktop.desktop "$scheme"
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$data_home/applications" || true
fi
printf 'Installed: %s\n' "$install_dir/gamenode"
printf 'URI handler: %s\n' "$desktop_file"
