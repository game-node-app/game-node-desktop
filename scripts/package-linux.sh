#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

stage=$(mktemp -d "${TMPDIR:-/tmp}/gamenode-linux-package.XXXXXX")
trap 'rm -rf "$stage"' EXIT
package="$stage/gamenode-desktop"
mkdir -p "$package/libexec"
# Capture identity before compilation, and pass the exact same value to the UI.
python3 - "$stage" <<'PY'
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import subprocess
import sys
import tomllib

stage = Path(sys.argv[1])
with Path("Cargo.toml").open("rb") as source:
    version = tomllib.load(source)["workspace"]["package"]["version"]
revision = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
dirty = bool(subprocess.check_output(
    ["git", "status", "--porcelain", "--untracked-files=all"], text=True
).strip())
built_at = datetime.now(timezone.utc)
# A timestamp also distinguishes successive builds from an uncommitted tree.
identity = f"g{revision[:12]}{'.dirty' if dirty else ''}.{built_at:%Y%m%dT%H%M%S%fZ}"
build_version = f"{version}{'.' if '+' in version else '+'}{identity}"
host = next(line.removeprefix("host: ") for line in subprocess.check_output(
    ["rustc", "-vV"], text=True
).splitlines() if line.startswith("host: "))
target = os.environ.get("CARGO_BUILD_TARGET") or host
if "/" in target or "linux" not in target:
    raise SystemExit("Linux packaging requires a Linux target triple, not a custom target file")
metadata = {
    "version": version,
    "buildVersion": build_version,
    "baseRevision": revision,
    "dirty": dirty,
    "builtAt": built_at.isoformat(),
    "target": target,
}
(stage / "gamenode-desktop/build-manifest.json").write_text(
    json.dumps(metadata, indent=2) + "\n", encoding="utf-8"
)
(stage / "gamenode-desktop/VERSION").write_text(build_version + "\n", encoding="utf-8")
(stage / "target").write_text(target + "\n", encoding="utf-8")
target_directory = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], text=True
))["target_directory"]
binary = Path(target_directory)
if os.environ.get("CARGO_BUILD_TARGET"):
    binary /= target
(stage / "binary").write_text(str(binary / "release/gamenode") + "\n", encoding="utf-8")
(stage / "plugin").write_text(str(binary / "release/gamenode-plugin-steam") + "\n", encoding="utf-8")
(stage / "packagekit-plugin").write_text(str(binary / "release/gamenode-plugin-packagekit") + "\n", encoding="utf-8")
PY
read -r build_version < "$package/VERSION"
read -r target < "$stage/target"
read -r binary < "$stage/binary"
read -r plugin < "$stage/plugin"
read -r packagekit_plugin < "$stage/packagekit-plugin"
pnpm --dir ui build
GAMENODE_BUILD_VERSION="$build_version" cargo build --release --bins -p gamenode-app -p gamenode-plugin-steam -p gamenode-plugin-packagekit --locked
cp "$binary" "$package/libexec/gamenode"
cp "$plugin" "$package/libexec/gamenode-plugin-steam"
cp "$packagekit_plugin" "$package/libexec/gamenode-plugin-packagekit"
cp LICENSE "$package/LICENSE"
cp packaging/linux/README.md "$package/README.md"
cp crates/app/icons/gamenode.png "$package/gamenode.png"
cp packaging/linux/{install.sh,uninstall.sh,gamenode.sh,desktop-integration.sh} "$package/"
cp -R licenses "$package/licenses"
chmod 755 "$package/libexec/gamenode" "$package/libexec/gamenode-plugin-steam" "$package/libexec/gamenode-plugin-packagekit" \
  "$package/install.sh" "$package/uninstall.sh" "$package/gamenode.sh"

mkdir -p dist/linux
python3 - "$package" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

package = Path(sys.argv[1])
manifest = package / "build-manifest.json"
metadata = json.loads(manifest.read_text(encoding="utf-8"))
metadata["filesSha256"] = {
    str(path.relative_to(package)): hashlib.sha256(path.read_bytes()).hexdigest()
    for path in sorted(package.rglob("*")) if path.is_file() and path != manifest
}
manifest.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")
PY
archive="dist/linux/gamenode-desktop-${build_version}-${target}.tar.gz"
archive_tmp="$archive.tmp.$$"
tar -C "$stage" -czf "$archive_tmp" gamenode-desktop
mv -f "$archive_tmp" "$archive"
python3 - "$archive" <<'PY'
import hashlib
from pathlib import Path
import shutil
import sys

archive = Path(sys.argv[1])
checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_name(archive.name + ".sha256").write_text(
    f"{checksum}  {archive.name}\n", encoding="utf-8"
)
# Compatibility alias for existing local instructions; prefer the versioned file.
alias = archive.with_name("gamenode-desktop.tar.gz")
temporary = alias.with_suffix(".gz.tmp")
shutil.copyfile(archive, temporary)
temporary.replace(alias)
PY
printf 'Version: %s\nPackage: %s\nLatest alias: %s\n' "$build_version" "$PWD/$archive" "$PWD/dist/linux/gamenode-desktop.tar.gz"
