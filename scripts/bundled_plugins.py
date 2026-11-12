#!/usr/bin/env python3
"""Read the trusted plugin descriptors and list the bundled plugin executables.

The descriptors in ``plugins/*.toml`` are embedded into the host at build time and are trusted.
This helper is the only place that maps a descriptor to a plugin executable name, so build, run,
package and verification scripts do not keep their own lists.

Requires Python 3.11+ for the standard-library ``tomllib`` module.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from functools import lru_cache
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from typing import Iterable, Sequence


REPO_ROOT = Path(__file__).resolve().parents[1]
DESCRIPTORS_DIR = REPO_ROOT / "plugins"
INDEX_FILE = "bundled-plugins.json"
KNOWN_PLATFORMS = ("linux", "windows")
OUTPUT_FORMATS = ("names", "files", "cargo-args", "json")
EXECUTABLE_PATTERN = re.compile(r"[a-z0-9-]+")


class BundledPluginsError(Exception):
    """Base error for an unusable bundled plugin set."""


class DescriptorError(BundledPluginsError):
    """A descriptor could not be read or does not declare a usable plugin."""


class MetadataError(BundledPluginsError):
    """Cargo metadata could not provide the workspace binary targets."""


@dataclass(frozen=True)
class Plugin:
    provider_id: str
    plugin_id: str
    executable: str
    platforms: tuple[str, ...]

    def file_name(self, platform: str) -> str:
        """Return the packaged file name for ``platform``."""
        if platform not in KNOWN_PLATFORMS:
            raise ValueError(f"unknown platform: {platform!r}")
        return f"{self.executable}.exe" if platform == "windows" else self.executable


def _string_field(data: dict, field: str, path: Path) -> str:
    value = data.get(field)
    if not isinstance(value, str) or not value.strip():
        raise DescriptorError(f"{path}: missing non-empty string field {field!r}")
    return value


def _platforms(data: dict, path: Path) -> tuple[str, ...]:
    value = data.get("platforms")
    if not isinstance(value, list) or not value:
        raise DescriptorError(f"{path}: missing non-empty list field 'platforms'")
    platforms: list[str] = []
    for platform in value:
        if not isinstance(platform, str) or platform not in KNOWN_PLATFORMS:
            raise DescriptorError(
                f"{path}: unknown platform {platform!r}; expected one of "
                f"{', '.join(KNOWN_PLATFORMS)}"
            )
        if platform in platforms:
            raise DescriptorError(f"{path}: duplicate platform {platform!r}")
        platforms.append(platform)
    return tuple(platforms)


def _read_descriptor(path: Path) -> tuple[Plugin, Path]:
    try:
        with path.open("rb") as source:
            data = tomllib.load(source)
    except tomllib.TOMLDecodeError as error:
        raise DescriptorError(f"{path}: invalid TOML: {error}") from error
    except OSError as error:
        raise DescriptorError(f"{path}: cannot read descriptor: {error}") from error

    provider_id = _string_field(data, "provider_id", path)
    plugin_id = _string_field(data, "plugin_id", path)
    executable = _string_field(data, "executable", path)
    if EXECUTABLE_PATTERN.fullmatch(executable) is None:
        raise DescriptorError(
            f"{path}: executable {executable!r} is not a plain file name matching [a-z0-9-]+"
        )
    platforms = _platforms(data, path)
    return Plugin(provider_id, plugin_id, executable, platforms), path


@lru_cache(maxsize=1)
def _cargo_binary_names() -> frozenset[str]:
    """Return every binary target name from ``cargo metadata`` without compiling the workspace."""
    try:
        result = subprocess.run(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
        )
    except OSError as error:
        raise MetadataError(f"could not run cargo metadata: {error}") from error
    if result.returncode != 0:
        raise MetadataError(f"cargo metadata failed: {result.stderr.strip()}")
    try:
        metadata = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise MetadataError(f"cargo metadata returned invalid JSON: {error}") from error
    names: set[str] = set()
    for package in metadata.get("packages", []):
        for target in package.get("targets", []):
            if "bin" in target.get("kind", []):
                names.add(target["name"])
    return frozenset(names)


def load(
    platform: str,
    descriptors_dir: Path | str | None = None,
    *,
    binary_names: Iterable[str] | None = None,
) -> list[Plugin]:
    """Return the plugins declared for ``platform``, sorted by provider id.

    ``binary_names`` and ``descriptors_dir`` exist so tests can use a temporary descriptor set
    without invoking Cargo.
    """
    if platform not in KNOWN_PLATFORMS:
        raise ValueError(
            f"unknown platform {platform!r}; expected one of {', '.join(KNOWN_PLATFORMS)}"
        )
    directory = Path(descriptors_dir) if descriptors_dir is not None else DESCRIPTORS_DIR
    if not directory.is_dir():
        raise DescriptorError(f"{directory}: plugin descriptor directory is missing")
    descriptors = sorted(directory.glob("*.toml"))
    declared: list[tuple[Plugin, Path]] = []
    provider_ids: dict[str, Path] = {}
    plugin_ids: dict[str, Path] = {}
    executables: dict[str, Path] = {}
    for descriptor in descriptors:
        plugin, path = _read_descriptor(descriptor)
        for value, seen, label in (
            (plugin.provider_id, provider_ids, "provider id"),
            (plugin.plugin_id, plugin_ids, "plugin id"),
            (plugin.executable, executables, "executable"),
        ):
            if value in seen:
                raise DescriptorError(
                    f"{path}: duplicate {label} {value!r} (also declared by {seen[value]})"
                )
            seen[value] = path
        declared.append((plugin, path))

    binaries = (
        frozenset(binary_names) if binary_names is not None else _cargo_binary_names()
    )
    for plugin, path in declared:
        if plugin.executable not in binaries:
            raise DescriptorError(
                f"{path}: executable {plugin.executable!r} is not a binary target in cargo metadata"
            )

    return sorted(
        (plugin for plugin, _ in declared if platform in plugin.platforms),
        key=lambda plugin: (plugin.provider_id, plugin.plugin_id),
    )


def write_index(
    package_dir: Path | str,
    platform: str,
    descriptors_dir: Path | str | None = None,
    *,
    binary_names: Iterable[str] | None = None,
) -> Path:
    """Write ``bundled-plugins.json`` for ``platform`` into ``package_dir``."""
    package = Path(package_dir)
    plugins = load(platform, descriptors_dir, binary_names=binary_names)
    index = {
        "schema": 1,
        "plugins": [
            {
                "providerId": plugin.provider_id,
                "pluginId": plugin.plugin_id,
                "file": plugin.file_name(platform),
            }
            for plugin in plugins
        ],
    }
    package.mkdir(parents=True, exist_ok=True)
    destination = package / INDEX_FILE
    destination.write_text(json.dumps(index, indent=2) + "\n", encoding="utf-8")
    return destination


def _json_payload(plugins: Sequence[Plugin]) -> list[dict]:
    return [
        {
            "provider_id": plugin.provider_id,
            "plugin_id": plugin.plugin_id,
            "executable": plugin.executable,
            "platforms": list(plugin.platforms),
        }
        for plugin in plugins
    ]


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True, choices=KNOWN_PLATFORMS)
    parser.add_argument(
        "--format", required=True, choices=OUTPUT_FORMATS, dest="output_format"
    )
    args = parser.parse_args(argv)
    try:
        plugins = load(args.platform)
    except BundledPluginsError as error:
        print(f"bundled_plugins: {error}", file=sys.stderr)
        return 1

    if args.output_format == "names":
        for plugin in plugins:
            print(plugin.executable)
    elif args.output_format == "files":
        for plugin in plugins:
            print(plugin.file_name(args.platform))
    elif args.output_format == "cargo-args":
        for plugin in plugins:
            print("--bin")
            print(plugin.executable)
    else:
        print(json.dumps(_json_payload(plugins), indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
