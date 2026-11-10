#!/usr/bin/env python3
"""Validate bundled host/plugin binaries and platform package resources without executing them."""

import argparse
from pathlib import Path
import struct
import sys


def _verify_linux_binary(binary: Path) -> list[str]:
    errors = []
    with binary.open("rb") as source:
        header = source.read(64)
    if len(header) < 20 or header[:6] != b"\x7fELF\x02\x01":
        errors.append(f"not an x86-64 Linux ELF binary: {binary}")
    elif struct.unpack_from("<H", header, 18)[0] != 62:
        errors.append(f"not an x86-64 Linux ELF binary: {binary}")
    if binary.stat().st_mode & 0o111 != 0o111:
        errors.append(f"binary is not executable by package users: {binary}")
    return errors


def _verify_windows_binary(binary: Path) -> list[str]:
    with binary.open("rb") as source:
        header = source.read(64)
        if len(header) != 64 or header[:2] != b"MZ":
            return [f"not an x86-64 Windows PE binary: {binary}"]
        offset = struct.unpack_from("<I", header, 60)[0]
        source.seek(offset)
        signature = source.read(6)
    if signature != b"PE\x00\x00\x64\x86":
        return [f"not an x86-64 Windows PE binary: {binary}"]
    return []


def verify(package: Path, platform: str) -> list[str]:
    package = Path(package)
    if platform == "linux":
        binary_dir = package / "libexec"
        names = ("gamenode", "gamenode-plugin-steam", "gamenode-plugin-packagekit")
        resources = (
            "LICENSE", "README.md", "build-manifest.json", "gamenode.png",
            "install.sh", "uninstall.sh", "gamenode.sh", "desktop-integration.sh",
        )
    else:
        binary_dir = package
        names = ("gamenode.exe", "gamenode-plugin-steam.exe", "gamenode-plugin-epic.exe")
        resources = (
            "LICENSE", "README.md", "build-manifest.json", "install.ps1", "uninstall.ps1",
            "backup-assets/build-inputs.json",
        )

    errors = []
    for name in names:
        binary = binary_dir / name
        if not binary.is_file() or binary.is_symlink():
            errors.append(f"missing regular binary: {binary}")
            continue
        errors.extend(
            _verify_linux_binary(binary) if platform == "linux" else _verify_windows_binary(binary)
        )
    for name in resources:
        if not (package / name).is_file():
            errors.append(f"missing package resource: {package / name}")
    licenses = package / "licenses"
    if not licenses.is_dir() or not any(path.is_file() for path in licenses.rglob("*")):
        errors.append(f"missing license notices: {licenses}")

    suffix = ".exe" if platform == "windows" else ""
    for obsolete in ("gamenode-cli", "gamenode-daemon"):
        if (binary_dir / (obsolete + suffix)).exists():
            errors.append(f"obsolete prototype binary in package: {obsolete}")
    if platform == "windows" and (binary_dir / "gamenode-plugin-packagekit.exe").exists():
        errors.append("Linux package-manager plugin must not be shipped in the Windows package")
    if platform == "linux":
        for epic in ("gamenode-plugin-epic", "gamenode-plugin-epic.exe"):
            if (binary_dir / epic).exists():
                errors.append(f"Windows-only Epic plugin must not be shipped in the Linux package: {epic}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", choices=("linux", "windows"), required=True)
    parser.add_argument("package", type=Path)
    args = parser.parse_args()
    errors = verify(args.package, args.platform)
    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print(f"{args.platform} host/plugin package layout passed: {args.package.resolve()}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
