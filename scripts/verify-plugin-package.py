#!/usr/bin/env python3
"""Validate bundled host/plugin binaries and platform package resources without executing them."""

import argparse
import json
from pathlib import Path
import struct
import sys


sys.path.insert(0, str(Path(__file__).resolve().parent))
import bundled_plugins  # noqa: E402  (the scripts directory is not a package)


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


def _verify_index(index: Path, plugins: list[bundled_plugins.Plugin], platform: str) -> list[str]:
    try:
        data = json.loads(index.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return [f"invalid bundled plugin index {index}: {error}"]
    entries = data.get("plugins") if isinstance(data, dict) else None
    if not isinstance(entries, list):
        return [f"invalid bundled plugin index {index}: expected a plugins list"]
    actual = set()
    for entry in entries:
        if not isinstance(entry, dict):
            return [f"invalid bundled plugin index {index}: expected plugin objects"]
        try:
            actual.add((entry["providerId"], entry["pluginId"], entry["file"]))
        except KeyError:
            return [f"invalid bundled plugin index {index}: missing plugin fields"]

    expected = {
        (plugin.provider_id, plugin.plugin_id, plugin.file_name(platform))
        for plugin in plugins
    }
    errors = []
    if data.get("schema") != 1:
        errors.append(f"bundled plugin index has an unsupported schema: {index}")
    if actual != expected or len(entries) != len(expected):
        errors.append(
            f"bundled plugin index does not match the descriptors for {platform}: {index}"
        )
    return errors


def _all_plugins() -> list[bundled_plugins.Plugin]:
    plugins = {}
    for plugin in (*bundled_plugins.load("linux"), *bundled_plugins.load("windows")):
        plugins[(plugin.provider_id, plugin.plugin_id)] = plugin
    return list(plugins.values())


def _other_platform_plugins(platform: str) -> list[bundled_plugins.Plugin]:
    return [plugin for plugin in _all_plugins() if platform not in plugin.platforms]


def verify(package: Path, platform: str) -> list[str]:
    package = Path(package)
    plugins = bundled_plugins.load(platform)
    plugin_files = [plugin.file_name(platform) for plugin in plugins]
    if platform == "linux":
        binary_dir = package / "libexec"
        names = ("gamenode", *plugin_files)
        resources = (
            "LICENSE", "README.md", "build-manifest.json", "gamenode.png",
            "install.sh", "uninstall.sh", "gamenode.sh", "desktop-integration.sh",
        )
    else:
        binary_dir = package
        names = ("gamenode.exe", *plugin_files)
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

    index = binary_dir / bundled_plugins.INDEX_FILE
    if not index.is_file() or index.is_symlink():
        errors.append(f"missing bundled plugin index: {index}")
    else:
        errors.extend(_verify_index(index, plugins, platform))

    suffix = ".exe" if platform == "windows" else ""
    for obsolete in ("gamenode-cli", "gamenode-daemon"):
        if (binary_dir / (obsolete + suffix)).exists():
            errors.append(f"obsolete prototype binary in package: {obsolete}")
    for plugin in _other_platform_plugins(platform):
        for name in (plugin.executable, plugin.executable + ".exe"):
            other = binary_dir / name
            if other.exists():
                errors.append(
                    f"plugin for another platform must not be shipped in the {platform} "
                    f"package: {other}"
                )

    known_plugin_names = set()
    for plugin in _all_plugins():
        known_plugin_names.add(plugin.executable)
        known_plugin_names.add(plugin.executable + ".exe")
    allowed = {*names, bundled_plugins.INDEX_FILE}
    for path in sorted(binary_dir.iterdir()):
        if not path.is_file():
            continue
        if platform == "windows" and path.suffix.lower() != ".exe":
            continue
        if path.name not in allowed and path.name not in known_plugin_names:
            errors.append(f"unexpected packaged executable: {path}")
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
