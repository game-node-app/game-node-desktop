#!/usr/bin/env python3
"""Build versioned, reproducible source bundles for the public plugin Rust crates."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
PACKAGES = {
    "gamenode-plugin-protocol": "crates/plugin-protocol",
    "gamenode-plugin-sdk": "crates/plugin-sdk",
}
LICENSE_PATTERNS = ("LICENSE*", "COPYING*", "NOTICE*", "PATENTS*")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def cargo_command(target_dir: Path, cwd: Path, *args: str) -> subprocess.CompletedProcess[str]:
    # The clean bundles live in a temporary directory; building them under the repository's
    # target directory keeps build output out of the archived sources.
    environment = os.environ.copy()
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    return subprocess.run(
        ["cargo", *args],
        cwd=cwd,
        env=environment,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=None,
    )


def repository_target_dir() -> Path:
    metadata = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout
    return Path(json.loads(metadata)["target_directory"]).resolve()


def unpack_crate(crate: Path, destination: Path) -> None:
    with tarfile.open(crate, mode="r:gz") as archive:
        for member in archive.getmembers():
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not name.parts:
                raise RuntimeError(f"unsafe entry in Cargo source archive: {member.name}")
            if member.issym() or member.islnk() or not (member.isfile() or member.isdir()):
                raise RuntimeError(f"unsupported entry in Cargo source archive: {member.name}")
            relative = Path(*name.parts[1:])
            if not relative.parts:
                continue
            output = destination / relative
            if not output.resolve().is_relative_to(destination.resolve()):
                raise RuntimeError(f"archive entry escaped extraction directory: {member.name}")
            if member.isdir():
                output.mkdir(parents=True, exist_ok=True)
                continue
            output.parent.mkdir(parents=True, exist_ok=True)
            source = archive.extractfile(member)
            if source is None:
                raise RuntimeError(f"could not read archive entry: {member.name}")
            with source, output.open("wb") as target:
                shutil.copyfileobj(source, target)
            output.chmod(member.mode & 0o777)


def dependency_notices(metadata: dict[str, Any], root: Path) -> str:
    packages = [
        item
        for item in metadata["packages"]
        if item.get("source") is not None
    ]
    packages.sort(key=lambda item: (item["name"].casefold(), item["version"]))
    lines = [
        "# Third-party dependency notices",
        "",
        "This inventory is generated from the locked source bundle with `cargo metadata`.",
        "License expressions are upstream metadata, not a legal review.",
        "",
        "| Crate | Version | Declared license | Included license text |",
        "|---|---:|---|---|",
    ]
    for package in packages:
        name = package["name"]
        version = package["version"]
        license_expression = package.get("license") or "Not declared"
        source_dir = Path(package["manifest_path"]).parent
        candidates: list[Path] = []
        specified = package.get("license_file")
        if specified:
            candidates.append(source_dir / specified)
        for pattern in LICENSE_PATTERNS:
            candidates.extend(source_dir.glob(pattern))
        files = sorted(
            {
                candidate
                for candidate in candidates
                if candidate.is_file() and not candidate.is_symlink() and candidate.stat().st_size <= 1024 * 1024
            }
        )
        copied: list[str] = []
        for license_file in files:
            relative = Path("licenses") / f"{name}-{version}" / license_file.name
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            if not target.exists():
                shutil.copyfile(license_file, target)
            copied.append(relative.as_posix())
        lines.append(
            f"| `{name}` | `{version}` | {license_expression} | "
            f"{', '.join(f'`{item}`' for item in copied) if copied else 'No license text file found in the crate source'} |"
        )
    lines.append("")
    return "\n".join(lines)


def reproducible_tar(source: Path, destination: Path, archive_root: str) -> None:
    temporary = destination.with_suffix(destination.suffix + ".partial")
    with temporary.open("wb") as output:
        import gzip

        with gzip.GzipFile(fileobj=output, mode="wb", mtime=0, compresslevel=9) as compressed:
            with tarfile.open(fileobj=compressed, mode="w") as archive:
                for path in sorted(source.rglob("*")):
                    if not path.is_file() or path.is_symlink():
                        continue
                    relative = path.relative_to(source)
                    entry_name = (Path(archive_root) / relative).as_posix()
                    info = archive.gettarinfo(str(path), arcname=entry_name)
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = 0
                    with path.open("rb") as stream:
                        archive.addfile(info, stream)
    temporary.replace(destination)


def build_bundle(
    package_name: str,
    crate_paths: dict[str, Path],
    output: Path,
    target_dir: Path,
) -> None:
    with tempfile.TemporaryDirectory(prefix=f"{package_name}-") as temporary:
        bundle = Path(temporary) / package_name
        crate_dirs: dict[str, Path] = {}
        for name, crate in crate_paths.items():
            subdir = "plugin-sdk" if name == "gamenode-plugin-sdk" else "plugin-protocol"
            destination = bundle / "crates" / subdir
            destination.mkdir(parents=True, exist_ok=True)
            unpack_crate(crate, destination)
            crate_dirs[name] = destination

        members = [f'"crates/{"plugin-sdk" if name == "gamenode-plugin-sdk" else "plugin-protocol"}"' for name in crate_dirs]
        workspace = "[workspace]\nresolver = \"3\"\nmembers = [" + ", ".join(members) + "]\n"
        if "gamenode-plugin-sdk" in crate_dirs:
            workspace += (
                "\n[patch.crates-io]\n"
                'gamenode-plugin-protocol = { path = "crates/plugin-protocol" }\n'
            )
        (bundle / "Cargo.toml").write_text(workspace, encoding="utf-8")
        shutil.copyfile(ROOT / "rust-toolchain.toml", bundle / "rust-toolchain.toml")

        cargo_command(
            target_dir,
            bundle,
            "generate-lockfile",
            "--offline",
            "--manifest-path",
            str(bundle / "Cargo.toml"),
        )
        metadata = json.loads(
            cargo_command(
                target_dir,
                bundle,
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--manifest-path",
                str(bundle / "Cargo.toml"),
            ).stdout
        )
        bundle_root = bundle.resolve()
        for package in metadata["packages"]:
            manifest_path = Path(package["manifest_path"]).resolve()
            if package["name"] in crate_dirs and not manifest_path.is_relative_to(bundle_root):
                raise RuntimeError(f"release package escaped its clean source bundle: {package['name']}")
            for dependency in package["dependencies"]:
                dependency_path = dependency.get("path")
                if dependency_path and not Path(dependency_path).resolve().is_relative_to(bundle_root):
                    raise RuntimeError(
                        f"dependency {package['name']} -> {dependency['name']} points outside the source bundle"
                    )

        sdk = crate_dirs.get("gamenode-plugin-sdk")
        if sdk is not None:
            sdk_manifest = tomllib.loads((sdk / "Cargo.toml").read_text(encoding="utf-8"))
            protocol_dependency = sdk_manifest.get("dependencies", {}).get("gamenode-plugin-protocol", {})
            protocol_version = tomllib.loads(
                (crate_dirs["gamenode-plugin-protocol"] / "Cargo.toml").read_text(encoding="utf-8")
            )["package"]["version"]
            if "path" in protocol_dependency or protocol_dependency.get("version") != f"={protocol_version}":
                raise RuntimeError("published SDK metadata must use the exact protocol version without a path")

        (bundle / "THIRD-PARTY-NOTICES.md").write_text(dependency_notices(metadata, bundle), encoding="utf-8")
        lock_path = bundle / "Cargo.lock"
        manifest = {
            "archive": package_name,
            "packages": [
                {"name": name, "version": tomllib.loads((crate_dir / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]}
                for name, crate_dir in crate_dirs.items()
            ],
            "rust_toolchain": (bundle / "rust-toolchain.toml").read_text(encoding="utf-8").strip(),
            "cargo_lock_sha256": sha256(lock_path),
            "package_dependencies": {
                package["name"]: [
                    {
                        "name": dep["name"],
                        "req": dep["req"],
                        "source": dep.get("source"),
                        "path": (
                            Path(dep["path"]).resolve().relative_to(bundle.resolve()).as_posix()
                            if dep.get("path") and Path(dep["path"]).resolve().is_relative_to(bundle.resolve())
                            else None
                        ),
                    }
                    for dep in package["dependencies"]
                ]
                for package in metadata["packages"]
                if package["name"] in crate_dirs
            },
        }
        (bundle / "release-manifest.json").write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )

        selected = [name for name in crate_dirs]
        cargo_args = ["test", "--locked", "--manifest-path", str(bundle / "Cargo.toml")]
        for name in selected:
            cargo_args.extend(["-p", name])
        if "gamenode-plugin-sdk" in crate_dirs:
            cargo_args.extend(["--lib", "--tests"])
        test_result = cargo_command(target_dir, bundle, *cargo_args)
        sys.stdout.write(test_result.stdout)

        version = next(
            item["version"] for item in manifest["packages"] if item["name"] == package_name
        )
        archive_path = output / f"{package_name}-{version}.tar.gz"
        reproducible_tar(bundle, archive_path, f"{package_name}-{version}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True, help="directory for versioned Cargo and source archives")
    parser.add_argument(
        "--allow-dirty-sources",
        action="store_true",
        help="allow local edits in the public crate sources (development verification only)",
    )
    args = parser.parse_args()

    with (ROOT / "Cargo.toml").open("rb") as stream:
        workspace = tomllib.load(stream)
    version = workspace["workspace"]["package"]["version"]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    target_dir = repository_target_dir()

    dirty = subprocess.run(
        ["git", "status", "--porcelain", "--", *PACKAGES.values()],
        cwd=ROOT,
        check=True,
        text=True,
        stdout=subprocess.PIPE,
    ).stdout
    if dirty and not args.allow_dirty_sources:
        parser.error("public crate sources have local changes; commit/review them or pass --allow-dirty-sources for verification")

    for package_name in PACKAGES:
        command = [
            "package",
            "--locked",
            "--no-verify",
            "--allow-dirty",
            "-p",
            package_name,
            "--target-dir",
            str(target_dir),
        ]
        if package_name == "gamenode-plugin-sdk":
            command.extend(
                ["--config", 'patch.crates-io.gamenode-plugin-protocol.path="crates/plugin-protocol"']
            )
        cargo_command(target_dir, ROOT, *command)

    crate_paths = {
        name: target_dir / "package" / f"{name}-{version}.crate"
        for name in PACKAGES
    }
    for crate in crate_paths.values():
        if not crate.is_file():
            raise RuntimeError(f"Cargo did not create the expected package archive: {crate}")
        shutil.copy2(crate, output / crate.name)

    build_bundle(
        "gamenode-plugin-protocol",
        {"gamenode-plugin-protocol": crate_paths["gamenode-plugin-protocol"]},
        output,
        target_dir / "plugin-source-bundles",
    )
    build_bundle(
        "gamenode-plugin-sdk",
        {
            "gamenode-plugin-protocol": crate_paths["gamenode-plugin-protocol"],
            "gamenode-plugin-sdk": crate_paths["gamenode-plugin-sdk"],
        },
        output,
        target_dir / "plugin-source-bundles",
    )
    checksums = []
    for artifact in sorted(output.glob("gamenode-plugin-*")):
        if artifact.is_file() and artifact.name != "SHA256SUMS":
            checksums.append(f"{sha256(artifact)}  {artifact.name}")
    (output / "SHA256SUMS").write_text("\n".join(checksums) + "\n", encoding="utf-8")
    print(f"Created Rust plugin release artifacts in {output}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError, tarfile.TarError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(1)
