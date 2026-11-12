#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

scripts/build-windows-wsl.sh
python3 - <<'PY'
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile

package = Path("dist/windows/gamenode-desktop")
with Path("Cargo.toml").open("rb") as manifest_file:
    version = tomllib.load(manifest_file)["workspace"]["package"]["version"]
archive = Path(f"dist/windows/gamenode-desktop-{version}-windows-x64.zip")
alias = Path("dist/windows/gamenode-desktop.zip")
source_files = sorted(str(path) for base in ("crates", "providers", "packaging", "scripts", "ui")
                      for path in Path(base).rglob("*") if path.is_file()
                      and not any(part in path.parts for part in ("__pycache__", "node_modules", "dist", ".turbo")))
sys.path.insert(0, str(Path("scripts").resolve()))
import bundled_plugins

plugins = bundled_plugins.load("windows")
plugin_files = [plugin.file_name("windows") for plugin in plugins]
if not plugin_files:
    raise SystemExit("No bundled plugins are declared for Windows")
metadata = json.loads(subprocess.check_output(
    ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"], text=True
))
binary_dir = Path(metadata["target_directory"]) / "x86_64-pc-windows-msvc" / "release"
for name in plugin_files:
    source = binary_dir / name
    if not source.is_file():
        raise SystemExit(f"Bundled plugin was not built: {source}")
    shutil.copyfile(source, package / name)
bundled_plugins.write_index(package, "windows")

binary = package / "gamenode.exe"
if not binary.is_file() or any(not (package / name).is_file() for name in plugin_files):
    raise SystemExit("Windows package is incomplete; build the host and bundled plugins before creating the ZIP")
revision = subprocess.run(
    ["git", "rev-parse", "HEAD"], check=True, capture_output=True, text=True
).stdout.strip()
dirty = bool(subprocess.run(
    ["git", "status", "--porcelain"], check=True, capture_output=True, text=True
).stdout.strip())
build_manifest = {
    "builtAt": datetime.now(timezone.utc).isoformat(),
    "baseRevision": revision,
    "dirty": dirty,
    "binarySha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
    "pluginSha256": {
        name: hashlib.sha256((package / name).read_bytes()).hexdigest()
        for name in plugin_files
    },
    "backupAssets": json.loads((package / "backup-assets/build-inputs.json").read_text()),
    "sourceSha256": {
        name: hashlib.sha256(Path(name).read_bytes()).hexdigest()
        for name in source_files
    },
}
(package / "build-manifest.json").write_text(
    json.dumps(build_manifest, indent=2) + "\n", encoding="utf-8"
)
files = [package / name for name in (
    "build-manifest.json", "gamenode.exe", *plugin_files, "bundled-plugins.json",
    "LICENSE", "README.md", "install.ps1", "uninstall.ps1",
)]
if any(not path.is_file() for path in files) or not (package / "licenses").is_dir():
    raise SystemExit("Windows package is incomplete; build it before creating the ZIP")
subprocess.run(
    ["python3", "scripts/verify-plugin-package.py", "--platform", "windows", str(package)],
    check=True,
)
archive.parent.mkdir(parents=True, exist_ok=True)
temporary = archive.with_suffix(".zip.tmp")
with ZipFile(temporary, "w", compression=ZIP_DEFLATED) as bundle:
    for path in files:
        bundle.write(path, path.relative_to(package.parent))
    for path in sorted([*(package / "licenses").rglob("*"), *(package / "backup-assets").rglob("*")]):
        if path.is_file():
            bundle.write(path, path.relative_to(package.parent))
temporary.replace(archive)
alias.write_bytes(archive.read_bytes())
print(f"Package: {archive.resolve()}")
print(f"Alias: {alias.resolve()}")
PY
