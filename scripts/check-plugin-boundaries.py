#!/usr/bin/env python3
"""Enforce production dependency and store boundaries for the bundled provider architecture."""

from __future__ import annotations

import argparse
from collections import deque
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from typing import NamedTuple


PUBLIC_PLUGIN_CRATES = (
    "gamenode-plugin-protocol",
    "gamenode-plugin-sdk",
    "gamenode-plugin-steam",
    "gamenode-plugin-packagekit",
    "gamenode-plugin-epic",
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
    "gamenode-plugin-epic",
}

REPO_ROOT = Path(__file__).resolve().parents[1]
ALLOWLIST_PATH = "scripts/plugin-boundary-allowlist.txt"

# Import the descriptor reader used by the build, packaging and verification scripts. The store
# vocabulary below is derived from it and the manifests, so adding a store needs no checker edit.
sys.path.insert(0, str(REPO_ROOT / "scripts"))
import bundled_plugins  # noqa: E402  (the scripts directory is not a package)

# Production sources that must stay store-agnostic. Stores reach the host only through plugin
# descriptors, manifests, scoped services and test fixtures.
STORE_SOURCE_ROOTS = (
    "crates/core/src",
    "crates/api/src",
    "crates/platform/src",
    "crates/app/src",
    "ui/src",
)
SOURCE_SUFFIXES = frozenset({".rs", ".ts", ".tsx", ".js", ".jsx"})
TEST_PATH_SEGMENTS = frozenset({"tests", "test", "__tests__", "fixtures"})
TEST_FILE_SUFFIXES = (".test.ts", ".test.tsx", ".test.js", ".test.jsx", ".spec.ts", ".spec.tsx")

RULE_NAMES = (
    "store-host",
    "store-module-path",
    "store-uri-scheme",
    "provider-id-literal",
    "store-identifier",
)
URI_SCHEME_PATTERN = re.compile(r"([A-Za-z][A-Za-z0-9+.-]*)://")


class StoreVocabularyError(Exception):
    """The bundled descriptors or manifests cannot provide the store vocabulary."""


class StoreVocabulary(NamedTuple):
    """Store literals derived from the trusted descriptors and provider manifests."""

    provider_ids: tuple[str, ...]
    domains: tuple[str, ...]
    uri_schemes: tuple[str, ...]


def _read_toml(path: Path) -> dict:
    try:
        with path.open("rb") as source:
            return tomllib.load(source)
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise StoreVocabularyError(f"{path}: {error}") from error


def _registrable_domain(host: str) -> str | None:
    """Return the last two labels of a host name, or None when there are fewer.

    Multi-part public suffixes such as `co.uk` are misclassified; this is a lint over our own
    trusted declarations, not a general URL classifier.
    """
    labels = [label for label in host.strip().lower().rstrip(".").split(".") if label]
    if len(labels) < 2:
        return None
    return ".".join(labels[-2:])


def _uri_scheme(template: str) -> str | None:
    match = URI_SCHEME_PATTERN.match(template)
    return match.group(1).lower() if match else None


def load_store_vocabulary(repo_root: Path) -> StoreVocabulary:
    """Derive provider ids, store hosts and URI schemes from trusted declarations."""
    repo_root = Path(repo_root)
    descriptors_dir = repo_root / "plugins"
    if not descriptors_dir.is_dir():
        raise StoreVocabularyError(f"plugin descriptor directory is missing: {descriptors_dir}")

    documents: list[tuple[Path, dict]] = [
        (path, _read_toml(path)) for path in sorted(descriptors_dir.glob("*.toml"))
    ]
    executables = {
        document["executable"]
        for _, document in documents
        if isinstance(document.get("executable"), str)
    }
    provider_ids: set[str] = set()
    try:
        for platform in bundled_plugins.KNOWN_PLATFORMS:
            provider_ids.update(
                plugin.provider_id
                for plugin in bundled_plugins.load(
                    platform, descriptors_dir, binary_names=executables
                )
            )
    except bundled_plugins.BundledPluginsError as error:
        raise StoreVocabularyError(str(error)) from error

    domains: set[str] = set()
    for path, document in documents:
        icon_hosts = document.get("achievement_icon_hosts") or []
        if not isinstance(icon_hosts, list):
            raise StoreVocabularyError(f"{path}: achievement_icon_hosts is not a list")
        for host in icon_hosts:
            if not isinstance(host, str):
                raise StoreVocabularyError(f"{path}: achievement icon host is not a string")
            domain = _registrable_domain(host)
            if domain is not None:
                domains.add(domain)

        policy_reference = document.get("login_policy")
        if isinstance(policy_reference, str) and policy_reference:
            policy = _read_toml(repo_root / policy_reference)
            allowed_hosts = policy.get("allowed_hosts") or []
            if not isinstance(allowed_hosts, list):
                raise StoreVocabularyError(
                    f"{repo_root / policy_reference}: allowed_hosts is not a list"
                )
            for host in allowed_hosts:
                if not isinstance(host, str):
                    raise StoreVocabularyError(
                        f"{repo_root / policy_reference}: allowed host is not a string"
                    )
                domain = _registrable_domain(host)
                if domain is not None:
                    domains.add(domain)

    uri_schemes: set[str] = set()
    providers_dir = repo_root / "providers"
    if providers_dir.is_dir():
        for path in sorted(providers_dir.glob("*.toml")):
            manifest = _read_toml(path)
            for key in ("launch", "install"):
                action = manifest.get(key)
                if not isinstance(action, dict):
                    continue
                template = action.get("uri")
                if not isinstance(template, str):
                    continue
                scheme = _uri_scheme(template)
                if scheme is not None:
                    uri_schemes.add(scheme)

    return StoreVocabulary(
        provider_ids=tuple(sorted(provider_ids)),
        domains=tuple(sorted(domains)),
        uri_schemes=tuple(sorted(uri_schemes)),
    )


def _alternation(values: tuple[str, ...]) -> str:
    if not values:
        return r"(?!)"
    return "(?:" + "|".join(re.escape(value) for value in values) + ")"


def source_rules(vocabulary: StoreVocabulary) -> dict[str, re.Pattern]:
    """Build the production source rules for one derived store vocabulary."""
    provider_ids = _alternation(vocabulary.provider_ids)
    return {
        "store-host": re.compile(_alternation(vocabulary.domains), re.IGNORECASE),
        "store-module-path": re.compile(rf"::{provider_ids}::"),
        # Quote-adjacent schemes only: backticked documentation examples may name a store.
        "store-uri-scheme": re.compile(
            rf"""["']{_alternation(vocabulary.uri_schemes)}://""", re.IGNORECASE
        ),
        "provider-id-literal": re.compile(rf"""["']{provider_ids}["']"""),
        "store-identifier": re.compile(rf"(?<![A-Za-z0-9]){provider_ids}_[A-Za-z0-9_]"),
    }


# Only an exact `#[cfg(test)]` (or `cfg(all(test, ...))`) item is test-only. `cfg(any(..., test))`
# also compiles production code and must stay visible to the scan.
CFG_TEST_ATTRIBUTE_PATTERN = re.compile(r"^\s*#\[cfg\(\s*(?:all\()?\s*test\s*[,)]")
ALLOWLIST_LINE_PATTERN = re.compile(
    r"^(?P<path>[^:\s]+):(?P<line>[1-9][0-9]*)\s+"
    r"(?P<rule>[a-z][a-z0-9-]*)\s+--\s+(?P<reason>\S.*)$"
)


class AllowlistEntry(NamedTuple):
    path: str
    line: int
    rule: str
    reason: str


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


def load_allowlist(
    path: Path, rule_names: tuple[str, ...] = RULE_NAMES
) -> tuple[list[AllowlistEntry], list[str]]:
    """Read reviewed path/line exceptions with a mandatory justification."""
    if not path.is_file():
        return [], [f"store boundary allowlist is missing: {path}"]
    entries: list[AllowlistEntry] = []
    errors: list[str] = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        match = ALLOWLIST_LINE_PATTERN.match(line)
        if match is None:
            errors.append(
                f"{path}:{number}: malformed store boundary allowlist entry; "
                "expected `<path>:<line> <rule> -- <justification>`"
            )
            continue
        rule = match.group("rule")
        if rule not in rule_names:
            errors.append(
                f"{path}:{number}: unknown store boundary rule `{rule}`; "
                f"expected one of {', '.join(sorted(rule_names))}"
            )
            continue
        entries.append(
            AllowlistEntry(
                path=match.group("path"),
                line=int(match.group("line")),
                rule=rule,
                reason=match.group("reason"),
            )
        )
    return entries, errors


def _is_test_path(relative: Path) -> bool:
    parts = relative.parts
    if any(part in TEST_PATH_SEGMENTS for part in parts[:-1]):
        return True
    if relative.name in {"tests.rs", "fixtures.rs"}:
        return True
    if relative.stem.endswith(("_tests", "_test")):
        return True
    if relative.name.endswith(TEST_FILE_SUFFIXES):
        return True
    if len(parts) >= 3 and parts[0] == "ui" and parts[1] == "src" and parts[2] == "mock":
        return True
    return False


_RUST_LITERAL_OR_COMMENT = re.compile(
    r'r#*".*?"#*'
    r'|"(?:\\.|[^"\\])*"'
    r"|'(?:\\.|[^'\\])'"
    r"|/\*.*?\*/"
    r"|//.*$"
)


def _test_item_end(lines: list[str], index: int) -> int:
    """Return the index after the item annotated by a `#[cfg(test)]` attribute.

    Tracking braces keeps fixture strings that end with `}` from ending the skip early, while a
    statement item ends at its semicolon.
    """
    depth = 0
    opened = False
    while index < len(lines):
        code = _RUST_LITERAL_OR_COMMENT.sub("", lines[index])
        depth += code.count("{") - code.count("}")
        if "{" in code:
            opened = True
        if opened and depth <= 0:
            return index + 1
        if not opened and lines[index].rstrip().endswith((";", ",")):
            return index + 1
        index += 1
    return index


def _production_lines(path: Path) -> list[tuple[int, str]]:
    """Return the lines the store scan may inspect, hiding `#[cfg(test)]` items in Rust."""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    if path.suffix != ".rs":
        return list(enumerate(lines, 1))

    visible: list[tuple[int, str]] = []
    index = 0
    while index < len(lines):
        if not CFG_TEST_ATTRIBUTE_PATTERN.match(lines[index]):
            visible.append((index + 1, lines[index]))
            index += 1
            continue
        index = _test_item_end(lines, index + 1)
    return visible


def _scan_source_file(
    repo_root: Path, path: Path, rules: dict[str, re.Pattern]
) -> list[tuple[str, int, str, str]]:
    relative = path.relative_to(repo_root).as_posix()
    findings = []
    for line_number, line in _production_lines(path):
        for rule, pattern in rules.items():
            if pattern.search(line):
                findings.append((relative, line_number, rule, line.strip()))
    return findings


def check_store_sources(repo_root: Path, allowlist_path: Path | None = None) -> list[str]:
    """Fail on store-specific production code outside trusted plugin data and test fixtures."""
    repo_root = Path(repo_root)
    allowlist_path = Path(allowlist_path) if allowlist_path else repo_root / ALLOWLIST_PATH
    try:
        vocabulary = load_store_vocabulary(repo_root)
    except StoreVocabularyError as error:
        return [f"store boundary vocabulary: {error}"]
    rules = source_rules(vocabulary)
    entries, errors = load_allowlist(allowlist_path, tuple(rules))
    allowed = {(entry.path, entry.line, entry.rule) for entry in entries}
    used: set[tuple[str, int, str]] = set()

    for root_name in STORE_SOURCE_ROOTS:
        root = repo_root / root_name
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*")):
            if not path.is_file() or path.suffix not in SOURCE_SUFFIXES:
                continue
            relative = path.relative_to(repo_root)
            if _is_test_path(relative):
                continue
            for relative_path, line_number, rule, text in _scan_source_file(
                repo_root, path, rules
            ):
                key = (relative_path, line_number, rule)
                if key in allowed:
                    used.add(key)
                    continue
                errors.append(
                    f"store boundary violation: {relative_path}:{line_number}: {rule}: {text}"
                )

    for entry in entries:
        if (entry.path, entry.line, entry.rule) not in used:
            errors.append(
                "stale store boundary allowlist entry: "
                f"{entry.path}:{entry.line}: {entry.rule} "
                f"({entry.reason!r}) has no matching production finding"
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
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            sys.stderr.write(result.stderr)
            return result.returncode
        metadata = json.loads(result.stdout)

    errors = check_metadata(metadata)
    errors.extend(check_store_sources(REPO_ROOT))
    if errors:
        for error in errors:
            if error.startswith(
                (
                    "store boundary violation:",
                    "store boundary vocabulary:",
                    "stale store boundary allowlist entry:",
                )
            ):
                print(error, file=sys.stderr)
            else:
                print(f"plugin boundary violation: {error}", file=sys.stderr)
        return 1
    print("plugin dependency and store boundaries are valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
