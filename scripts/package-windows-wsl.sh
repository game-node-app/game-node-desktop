#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

scripts/build-windows-wsl.sh
python3 - <<'PY'
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib
from zipfile import ZIP_DEFLATED, ZipFile

package = Path("dist/windows/gamenode-desktop")
with Path("Cargo.toml").open("rb") as manifest_file:
    version = tomllib.load(manifest_file)["workspace"]["package"]["version"]
archive = Path(f"dist/windows/gamenode-desktop-{version}-windows-x64.zip")
alias = Path("dist/windows/gamenode-desktop.zip")
source_files = [
    "crates/app/Cargo.toml",
    "crates/app/build.rs",
    "crates/app/examples/ui-review.rs",
    "crates/app/src/engine/mod.rs",
    "crates/app/src/engine/tests.rs",
    "crates/app/src/lib.rs",
    "crates/app/src/main.rs",
    "crates/app/ui/app.slint",
    "crates/app/ui/controls.slint",
    "crates/app/ui/fonts/Roboto-Variable.ttf",
    "crates/app/ui/icons/arrow.svg",
    "crates/app/ui/icons/gamenode-logo.svg",
    "crates/app/ui/icons/gamenode.ico",
    "crates/app/ui/icons/gamenode.png",
    "crates/app/ui/icons/library.svg",
    "crates/app/ui/icons/settings.svg",
    "crates/app/ui/theme-provenance.json",
    "crates/app/ui/theme.slint",
]
binary = package / "gamenode.exe"
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
    "sourceSha256": {
        name: hashlib.sha256(Path(name).read_bytes()).hexdigest()
        for name in source_files
    },
}
(package / "build-manifest.json").write_text(
    json.dumps(build_manifest, indent=2) + "\n", encoding="utf-8"
)
files = [package / name for name in (
    "build-manifest.json", "gamenode.exe", "LICENSE", "README.md",
    "install.ps1", "uninstall.ps1",
)]
if any(not path.is_file() for path in files) or not (package / "licenses").is_dir():
    raise SystemExit("Windows package is incomplete; build it before creating the ZIP")
archive.parent.mkdir(parents=True, exist_ok=True)
temporary = archive.with_suffix(".zip.tmp")
with ZipFile(temporary, "w", compression=ZIP_DEFLATED) as bundle:
    for path in files:
        bundle.write(path, path.relative_to(package.parent))
    for path in sorted((package / "licenses").rglob("*")):
        if path.is_file():
            bundle.write(path, path.relative_to(package.parent))
temporary.replace(archive)
alias.write_bytes(archive.read_bytes())
print(f"Package: {archive.resolve()}")
print(f"Alias: {alias.resolve()}")
PY
