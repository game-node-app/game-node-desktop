#!/usr/bin/env bash
# Shared by the launcher and installer. Never source installation metadata.

integration_error() {
    local message="GameNode desktop integration: $*"
    printf '%s\n' "$message" >&2
    # Desktop-file launches have no terminal. A failed/unavailable dialog must
    # never turn a registration error into a successful launch.
    if [[ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ]]; then
        if command -v kdialog >/dev/null; then
            if kdialog --title 'GameNode Desktop' --error "$message"; then
                return 1
            fi
        fi
        if command -v zenity >/dev/null; then
            zenity --error --no-markup --title='GameNode Desktop' --text="$message" || true
        fi
    fi
    return 1
}

integration_set_default() {
    # Some xdg-utils versions call the unversioned qtpaths outside Qt's PATH.
    # Scope this adjustment to xdg-mime; do not change the application environment.
    if [[ "${KDE_SESSION_VERSION:-0}" == 6 ]] && ! command -v qtpaths >/dev/null; then
        if [[ -x /usr/lib/qt6/bin/qtpaths ]]; then
            PATH="/usr/lib/qt6/bin:$PATH" xdg-mime default "$1" "$scheme"
        else
            integration_error 'xdg-mime needs qtpaths in this KDE session. Check your Qt 6 installation and PATH, then retry.'
            return 1
        fi
    else
        xdg-mime default "$1" "$scheme"
    fi
}

integration_handler_exists() {
    local id="$1" directory
    local -a system_dirs
    [[ "$id" == *.desktop && "$id" != */* ]] || return 1
    [[ ! -f "$data_home/applications/$id" ]] || return 0
    IFS=: read -r -a system_dirs <<< "${XDG_DATA_DIRS:-/usr/local/share:/usr/share}"
    for directory in "${system_dirs[@]}"; do
        [[ "$directory" != /* || ! -f "$directory/applications/$id" ]] || return 0
    done
    return 1
}

integration_prepare() {
    if [[ "$integration_dir" == *$'\n'* || "$integration_dir" == *$'\r'* || "$integration_dir" == *$'\t'* || "$integration_dir" == *\\* || "$integration_dir" == *%* ]]; then
        integration_error 'Use a package path without percent signs, backslashes, tabs or line breaks.'
        return 1
    fi
    command -v xdg-mime >/dev/null || {
        integration_error 'Install xdg-utils (Arch: sudo pacman -S xdg-utils), then run the launcher again.'
        return 1
    }
    data_home=${XDG_DATA_HOME:-"$HOME/.local/share"}
    config_home=${XDG_CONFIG_HOME:-"$HOME/.config"}
    [[ "$data_home" == /* && "$config_home" == /* ]] || {
        integration_error 'XDG_DATA_HOME and XDG_CONFIG_HOME must be absolute paths.'
        return 1
    }
    local path
    for path in "$data_home" "$config_home"; do
        if [[ "$path" == *$'\n'* || "$path" == *$'\r'* || "$path" == *$'\t'* || "$path" == *\\* ]]; then
            integration_error 'Use XDG paths without backslashes, tabs or line breaks.'
            return 1
        fi
    done
    export XDG_DATA_HOME="$data_home" XDG_CONFIG_HOME="$config_home"
    desktop_file="$data_home/applications/gamenode-desktop.desktop"
    scheme='x-scheme-handler/app.gamenode'
    [[ ! -L "$desktop_file" ]] || {
        integration_error "Refusing symlink desktop entry: $desktop_file"
        return 1
    }
    if [[ -e "$desktop_file" ]]; then
        grep -Fxq 'X-GameNode-Desktop-Owner=app.gamenode.desktop' "$desktop_file" || {
            integration_error "Refusing an unowned desktop entry: $desktop_file"
            return 1
        }
        registered_dir=$(sed -n 's/^X-GameNode-Desktop-Directory=//p' "$desktop_file")
        if [[ -z "$registered_dir" || ( "$registered_dir" != "$integration_dir" && -e "$registered_dir" ) ]]; then
            integration_error 'Another installation owns this desktop entry. Remove its integration with its uninstall.sh before switching installations.'
            return 1
        fi
    fi
    current_handler=$(xdg-mime query default "$scheme") || {
        integration_error 'Could not query the protocol association. Check your desktop session and retry.'
        return 1
    }
    previous_handler=''
    if [[ -f "$integration_dir/.gamenode-desktop-install" && ! -L "$integration_dir/.gamenode-desktop-install" ]] &&
       grep -Fxq 'product=app.gamenode.desktop' "$integration_dir/.gamenode-desktop-install"; then
        previous_handler=$(sed -n 's/^previous-handler=//p' "$integration_dir/.gamenode-desktop-install")
    fi
    if [[ -n "$current_handler" && "$current_handler" != 'gamenode-desktop.desktop' ]]; then
        if [[ "${integration_force:-false}" != true ]]; then
            integration_error "Protocol already belongs to $current_handler.
To explicitly replace it, rerun with --force:
  bash ./gamenode.sh --force   (or bash ./install.sh --force for stable installation)
No manual MIME edits are needed. Without --force, the current association is preserved."
            return 1
        fi
        previous_handler=$current_handler
        printf 'GameNode: replacing protocol association %s for this user (--force).\n' "$current_handler" >&2
    fi
}

integration_register() {
    # Desktop Entry Exec has its own escaping rules, in addition to string escapes.
    local exec_path="$integration_dir/gamenode.sh" icon_path="$integration_dir/gamenode.png"
    local entry_tmp marker_tmp entry_changed=false
    exec_path=${exec_path//\\/\\\\\\\\}
    exec_path=${exec_path//\"/\\\\\"}
    exec_path=${exec_path//\$/\\\\\$}
    exec_path=${exec_path//\`/\\\\\`}
    exec_path=${exec_path//%/%%}
    icon_path=${icon_path//\\/\\\\}
    mkdir -p "$data_home/applications" "$config_home"
    entry_tmp=$(mktemp "$data_home/applications/.gamenode-desktop.XXXXXX")
    # A fixed first executable also lets older xdg-mime resolve paths with spaces.
    cat > "$entry_tmp" <<EOF
[Desktop Entry]
Type=Application
Name=GameNode Desktop
Comment=Secure GameNode account sign-in
Exec=/usr/bin/env "$exec_path" %u
Icon=$icon_path
Terminal=false
Categories=Game;
MimeType=x-scheme-handler/app.gamenode;
StartupNotify=true
X-GameNode-Desktop-Owner=app.gamenode.desktop
X-GameNode-Desktop-Directory=$integration_dir
EOF
    chmod 644 "$entry_tmp"
    if [[ -f "$desktop_file" ]] && cmp -s "$entry_tmp" "$desktop_file"; then
        rm -f "$entry_tmp"
    else
        mv -f "$entry_tmp" "$desktop_file"
        entry_changed=true
    fi
    marker_tmp=$(mktemp "$integration_dir/.gamenode-desktop-install.XXXXXX")
    printf 'product=app.gamenode.desktop\ndata-home=%s\nconfig-home=%s\nmode=%s\nprevious-handler=%s\n' \
        "$data_home" "$config_home" "$integration_mode" "$previous_handler" > "$marker_tmp"
    chmod 600 "$marker_tmp"
    if [[ -f "$integration_dir/.gamenode-desktop-install" ]] &&
       cmp -s "$marker_tmp" "$integration_dir/.gamenode-desktop-install"; then
        rm -f "$marker_tmp"
    else
        mv -f "$marker_tmp" "$integration_dir/.gamenode-desktop-install"
    fi
    if [[ "$entry_changed" == true || "${integration_force:-false}" == true ]] && command -v update-desktop-database >/dev/null; then
        update-desktop-database "$data_home/applications" || true
    fi
    if [[ "$current_handler" != 'gamenode-desktop.desktop' || "${integration_force:-false}" == true ]]; then
        integration_set_default gamenode-desktop.desktop || {
            integration_error 'Could not register the protocol. Check directory permissions and rerun the launcher.'
            return 1
        }
    fi
    [[ "$(xdg-mime query default "$scheme")" == 'gamenode-desktop.desktop' ]] || {
        integration_error 'Protocol verification failed. Check desktop-specific MIME defaults and rerun the launcher.'
        return 1
    }
}
