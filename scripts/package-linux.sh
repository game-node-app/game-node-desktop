#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

cargo build --release -p gamenode-app --locked
stage=$(mktemp -d "${TMPDIR:-/tmp}/gamenode-linux-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT
package="$stage/gamenode-desktop"
mkdir -p "$package"
cp target/release/gamenode "$package/gamenode"
cp LICENSE "$package/LICENSE"
cp packaging/linux/README.md "$package/README.md"
cp crates/app/ui/icons/gamenode.png "$package/gamenode.png"
cp packaging/linux/install.sh packaging/linux/uninstall.sh "$package/"
cp -R licenses "$package/licenses"
chmod 755 "$package/gamenode" "$package/install.sh" "$package/uninstall.sh"

mkdir -p dist/linux
archive_tmp="dist/linux/gamenode-desktop.tar.gz.tmp.$$"
tar -C "$stage" -czf "$archive_tmp" gamenode-desktop
mv -f "$archive_tmp" dist/linux/gamenode-desktop.tar.gz
printf 'Package: %s\n' "$PWD/dist/linux/gamenode-desktop.tar.gz"
