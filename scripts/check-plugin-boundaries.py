#!/usr/bin/env python3
"""Enforce production dependency boundaries for the bundled provider architecture."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import sys
from collections import deque


PUBLIC_PLUGIN_CRATES = (
    "gamenode-plugin-protocol",
    "gamenode-plugin-sdk",
    "gamenode-plugin-steam",
    "gamenode-plugin-packagekit",
)
INTERNAL_APPLICATION_CRATES = {
    "gamenode-app",
    "gamenode-api",
    "gamenode-core",
    "gamenode-platform",
}
FORBIDDEN_HOST_PLUGIN_CRATES = {
    "gamenode-plugin-sdk",
    "gamenode-plugin-steam",
    "gamenode-plugin-packagekit",
}


def _dependency_kinds(edge: dict) -> list[dict]:
    return edge.get("dep_kinds") or [{"kind": None, "target": None}]


def _production_edge(edge: dict) -> bool:
    """Include normal and build edges, including target-specific declarations."""
    return any(kind.get("kind") != "dev" for kind in _dependency_kinds(edge))


def _all_edges(edge: dict) -> bool:
    return bool(_dependency_kinds(edge))


def _closure(metadata: dict, root_name: str, *, production_only: bool):
    packages_by_id = {package["id"]: package for package in metadata["packages"]}
    package_ids_by_name: dict[str, list[str]] = {}
    for package in metadata["packages"]:
        package_ids_by_name.setdefault(package["name"], []).append(package["id"])
    if root_name not in package_ids_by_name:
        return [], f"workspace package `{root_name}` is missing"
    nodes = {node["id"]: node for node in metadata.get("resolve", {}).get("nodes", [])}
    # Workspace package names are unique in this workspace; fail clearly if a future
    # version adds a duplicate instead of silently checking the wrong node.
    roots = package_ids_by_name[root_name]
    if len(roots) != 1:
        return [], f"workspace package `{root_name}` has ambiguous package IDs"

    queue = deque([(roots[0], [root_name])])
    visited = {roots[0]}
    reachable = []
    while queue:
        package_id, path = queue.popleft()
        node = nodes.get(package_id)
        if node is None:
            continue
        for edge in node.get("deps", []):
            if production_only and not _production_edge(edge):
                continue
            if not production_only and not _all_edges(edge):
                continue
            dependency_id = edge["pkg"]
            dependency = packages_by_id.get(dependency_id)
            if dependency is None:
                continue
            dependency_name = dependency["name"]
            kinds = _dependency_kinds(edge)
            kinds_text = ", ".join(
                f"{kind.get('kind') or 'normal'}"
                + (f" target {kind['target']}" if kind.get("target") else "")
                for kind in kinds
            )
            next_path = path + [dependency_name]
            reachable.append((dependency_name, next_path, kinds_text))
            if dependency_id not in visited:
                visited.add(dependency_id)
                queue.append((dependency_id, next_path))
    return reachable, None


def check_metadata(metadata: dict) -> list[str]:
    errors = []
    for crate in PUBLIC_PLUGIN_CRATES:
        reachable, error = _closure(metadata, crate, production_only=False)
        if error:
            errors.append(error)
            continue
        for name, path, kinds in reachable:
            if name in INTERNAL_APPLICATION_CRATES:
                errors.append(
                    f"{crate} reaches forbidden internal crate `{name}` via "
                    f"{' -> '.join(path)} ({kinds})"
                )

    reachable, error = _closure(metadata, "gamenode-app", production_only=True)
    if error:
        errors.append(error)
    else:
        for name, path, kinds in reachable:
            if name in FORBIDDEN_HOST_PLUGIN_CRATES:
                errors.append(
                    f"gamenode-app production graph reaches plugin implementation/client "
                    f"`{name}` via {' -> '.join(path)} ({kinds}); keep plugin crates test-only"
                )
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--metadata-file",
        type=Path,
        help="read cargo metadata JSON from a file instead of invoking Cargo (for fixtures)",
    )
    args = parser.parse_args()
    if args.metadata_file:
        metadata = json.loads(args.metadata_file.read_text(encoding="utf-8"))
    else:
        result = subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=Path(__file__).resolve().parents[1],
            check=True,
            capture_output=True,
            text=True,
        )
        metadata = json.loads(result.stdout)

    errors = check_metadata(metadata)
    if errors:
        for error in errors:
            print(f"plugin boundary violation: {error}", file=sys.stderr)
        return 1
    print("plugin dependency boundaries are valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
