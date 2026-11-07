#!/usr/bin/env bash
set -euo pipefail

integration_force=false
case "${1:-}" in
    --force) integration_force=true; shift ;;
    --help)
        echo 'Usage: ./gamenode.sh [--force] [--] [application arguments]'
        echo '  --force  Explicitly replace the current protocol association for this user.'
        echo '           Does not overwrite unowned desktop files or another live installation.'
        exit 0 ;;
esac
[[ "${1:-}" != -- ]] || shift
integration_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
source "$integration_dir/desktop-integration.sh"
integration_mode=portable
[[ -x "$integration_dir/libexec/gamenode" ]] || {
    integration_error 'GameNode internal binary (libexec/gamenode) is missing or not executable. Extract the complete package again.'
    exit 1
}
[[ -x "$integration_dir/libexec/gamenode-plugin-steam" ]] || {
    integration_error 'Official Steam discovery plugin (libexec/gamenode-plugin-steam) is missing or not executable. Extract the complete package again.'
    exit 1
}
# Preserve installed mode when launched from the stable installation.
if [[ -f "$integration_dir/.gamenode-desktop-install" ]] &&
   grep -Fxq 'mode=installed' "$integration_dir/.gamenode-desktop-install"; then
    integration_mode=installed
fi
integration_prepare
integration_register
exec "$integration_dir/libexec/gamenode" "$@"
