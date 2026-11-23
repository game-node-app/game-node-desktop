#!/usr/bin/env python3
"""Conformance checks for the third-party plugin protocol surface and its examples.

These checks are intentionally independent of the Rust build: they can be run before
committing a protocol or example change. They cover:

* public dependency boundaries for the Rust example (no internal Core crates);
* the API window and identities declared by the example manifest and source constants;
* the ``DescribeApi`` route allowlist parsed from the host's served-method tables.

Nothing here executes plugin code. Exit status is non-zero when a check fails.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
PROTO = REPO_ROOT / "crates/plugin-protocol/proto/plugin.proto"
PROTOCOL_LIB = REPO_ROOT / "crates/plugin-protocol/src/lib.rs"
PROTOCOL_CAPABILITY = REPO_ROOT / "crates/plugin-protocol/src/capability.rs"
PLUGIN_HOST = REPO_ROOT / "crates/app/src/plugin_host.rs"
ACCEPTANCE = REPO_ROOT / "docs/plugin-endpoint-acceptance.md"
RESOURCES = REPO_ROOT / "crates/app/src/plugin_host/resources.rs"
SAVES = REPO_ROOT / "crates/app/src/plugin_host/resources/saves.rs"
LOCATIONS = REPO_ROOT / "crates/app/src/plugin_host/resources/locations.rs"

INTERNAL_CRATES = {
    "gamenode-core",
    "gamenode-platform",
    "gamenode-app",
    "gamenode-api",
    "gamenode-plugin-protocol",
    "gamenode-plugin-steam",
    "gamenode-plugin-packagekit",
    "gamenode-plugin-epic",
}
# Rust identifiers of the same crates, as written in `use` paths.
INTERNAL_IDENTIFIERS = {
    "gamenode_core",
    "gamenode_platform",
    "gamenode_app",
    "gamenode_api",
    "gamenode_plugin_protocol",
    "gamenode_plugin_steam",
    "gamenode_plugin_packagekit",
    "gamenode_plugin_epic",
}


class Problems(list[str]):
    """A check's failures."""

    def fail(self, message: str) -> None:
        self.append(message)


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# Public dependency boundaries
# ---------------------------------------------------------------------------


def check_rust_example(problems: Problems) -> None:
    root = REPO_ROOT / "examples/plugins/rust-sdk"
    manifest = tomllib.loads(read(root / "Cargo.toml"))
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        for name, value in manifest.get(section, {}).items():
            if name in INTERNAL_CRATES:
                problems.fail(f"{root.name} depends on internal crate {name}")
            if isinstance(value, dict) and ("path" in value or "git" in value):
                problems.fail(
                    f"{root.name} dependency {name} is not a registry dependency; "
                    "the example must build from the published SDK"
                )
            if name == "gamenode-plugin-sdk" and value != "=0.1.0":
                problems.fail(
                    f"{root.name} must depend on the exact published SDK (=0.1.0), "
                    f"found {value!r}"
                )
    allowed = {"gamenode_plugin_sdk", "gamenode_plugin_mvp_example"}
    for path in sorted(root.rglob("*.rs")):
        for identifier in re.findall(r"\bgamenode_[a-z_]+\b", read(path)):
            if identifier not in allowed:
                problems.fail(
                    f"{path.relative_to(REPO_ROOT)} uses internal identifier {identifier}"
                )


def check_boundaries() -> tuple[Problems, list[str]]:
    problems = Problems()
    check_rust_example(problems)
    return problems, []


# ---------------------------------------------------------------------------
# API window and identities
# ---------------------------------------------------------------------------


def protocol_constants() -> dict[str, int]:
    values: dict[str, int] = {}
    for name, path in (
        ("API_MAJOR", PROTOCOL_LIB),
        ("API_MINOR", PROTOCOL_LIB),
        ("MIN_API_MINOR", PROTOCOL_LIB),
        ("THIRD_PARTY_API_MINOR", PROTOCOL_CAPABILITY),
    ):
        match = re.search(rf"pub const {name}: u32 = (\d+);", read(path))
        if not match:
            raise RuntimeError(f"{path.relative_to(REPO_ROOT)} has no {name}")
        values[name] = int(match.group(1))
    return values


def manifest(path: Path) -> dict:
    return json.loads(read(path))


def compare_identity(
    problems: Problems, manifest_path: Path, data: dict, plugin_id: str, provider_id: str
) -> None:
    relative = manifest_path.relative_to(REPO_ROOT)
    if data.get("id") != plugin_id:
        problems.fail(f"{relative} id {data.get('id')!r} != source {plugin_id!r}")
    if data.get("provider_id") != provider_id:
        problems.fail(
            f"{relative} provider_id {data.get('provider_id')!r} != source {provider_id!r}"
        )


def compare_api(
    problems: Problems, manifest_path: Path, data: dict, constants: dict[str, int]
) -> None:
    relative = manifest_path.relative_to(REPO_ROOT)
    api = data.get("api", {})
    expected_min = f"{constants['API_MAJOR']}.{constants['THIRD_PARTY_API_MINOR']}.0"
    expected_max = f"{constants['API_MAJOR']}.{constants['API_MINOR']}.0"
    if api.get("min") != expected_min:
        problems.fail(f"{relative} api.min {api.get('min')!r} != host minimum {expected_min!r}")
    if api.get("max") != expected_max:
        problems.fail(f"{relative} api.max {api.get('max')!r} != host maximum {expected_max!r}")


def check_versions() -> tuple[Problems, list[str]]:
    problems = Problems()
    constants = protocol_constants()
    if constants["THIRD_PARTY_API_MINOR"] > constants["API_MINOR"]:
        problems.fail("the third-party API minimum is above the current API minor")
    if constants["MIN_API_MINOR"] > constants["THIRD_PARTY_API_MINOR"]:
        problems.fail("the legacy API minimum is above the third-party minimum")

    rust_source = read(REPO_ROOT / "examples/plugins/rust-sdk/src/lib.rs")
    rust_plugin = re.search(r'PLUGIN_ID: &str = "([^"]+)"', rust_source)
    rust_provider = re.search(r'PROVIDER_ID: &str = "([^"]+)"', rust_source)
    if not rust_plugin or not rust_provider:
        problems.fail("the Rust example must declare PLUGIN_ID and PROVIDER_ID")
    else:
        compare_identity(
            problems,
            REPO_ROOT / "examples/plugins/rust-sdk/package/plugin.json",
            manifest(REPO_ROOT / "examples/plugins/rust-sdk/package/plugin.json"),
            rust_plugin.group(1),
            rust_provider.group(1),
        )

    relative = "examples/plugins/rust-sdk/package/plugin.json"
    path = REPO_ROOT / relative
    compare_api(problems, path, manifest(path), constants)
    version = manifest(path).get("version", "")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        problems.fail(f"{relative} version {version!r} is not SemVer")
    return problems, []


# ---------------------------------------------------------------------------
# DescribeApi route allowlist
# ---------------------------------------------------------------------------

SERVED_TABLES = (
    ("ResourceHost", RESOURCES, "SERVED"),
    ("Saves", SAVES, "SERVED"),
    ("Locations", LOCATIONS, "SERVED"),
)


def proto_services() -> dict[str, list[str]]:
    return parse_proto_services(read(PROTO))


def parse_proto_services(source: str) -> dict[str, list[str]]:
    services: dict[str, list[str]] = {}
    for match in re.finditer(r"service\s+(\w+)\s*\{(.*?)\n\}", source, re.S):
        services[match.group(1)] = re.findall(r"rpc\s+(\w+)", match.group(2))
    return services


def served_methods(path: Path, constant: str) -> list[str]:
    return parse_served(read(path), constant)


def parse_served(source: str, constant: str) -> list[str]:
    match = re.search(
        rf"pub const {constant}: &\[Served\] = &\[(.*?)\n\];", source, re.S
    )
    if not match:
        return []
    return re.findall(r'Served::new\(\s*"([A-Za-z]+)"', match.group(1))


def plugin_host_routes() -> list[str]:
    return parse_plugin_host_routes(read(PLUGIN_HOST))


def parse_plugin_host_routes(source: str) -> list[str]:
    return re.findall(
        r'service:\s*"PluginHost"\.into\(\),\s*method:\s*"([A-Za-z]+)"', source
    )


def advertised_routes() -> dict[str, list[str]]:
    routes = {
        service: served_methods(path, constant)
        for service, path, constant in SERVED_TABLES
    }
    routes["PluginHost"] = plugin_host_routes()
    return routes


def check_endpoints() -> tuple[Problems, list[str]]:
    problems = Problems()
    services = proto_services()
    advertised = advertised_routes()
    notes = []
    for service, methods in advertised.items():
        if service not in services:
            problems.fail(f"DescribeApi advertises unknown service {service}")
            continue
        if not methods:
            problems.fail(f"no served {service} methods were found")
        duplicates = {method for method in methods if methods.count(method) > 1}
        for method in sorted(duplicates):
            problems.fail(f"{service}.{method} is advertised more than once")
        for method in methods:
            if method not in services[service]:
                problems.fail(f"{service}.{method} is advertised but absent from plugin.proto")
    if set(advertised["PluginHost"]) != {
        "DescribeApi",
        "GetPermissions",
        "WatchPermissionChanges",
    }:
        problems.fail("DescribeApi must advertise exactly the three session methods it serves")
    deferred = sorted(
        f"{service}.{method}"
        for service, methods in services.items()
        if service in advertised
        for method in methods
        if method not in advertised[service]
    )
    notes.append(f"{sum(map(len, advertised.values()))} routes advertised, {len(deferred)} deferred")
    return problems, notes


# ---------------------------------------------------------------------------


def check_acceptance() -> tuple[Problems, list[str]]:
    """Every advertised route must be classified on the acceptance page.

    The page groups routes by family instead of one row per route, so a route is
    classified when its method name appears there. An unclassified route is an
    acceptance gap, not a documentation detail: the endpoint shipped without an
    owner or a status.
    """
    problems = Problems()
    notes: list[str] = []
    advertised = advertised_routes()
    services = proto_services()
    page = read(ACCEPTANCE)
    missing = sorted(
        f"{service}.{method}"
        for service, methods in advertised.items()
        for method in methods
        if method not in page
    )
    for route in missing:
        problems.fail(f"{route} is advertised but absent from docs/plugin-endpoint-acceptance.md")
    deferred = sorted(
        f"{service}.{method}"
        for service, methods in services.items()
        if service in advertised
        for method in methods
        if method not in advertised[service]
    )
    undocumented = [
        route for route in deferred if route.split(".")[1] not in page
    ]
    for route in undocumented:
        problems.fail(f"{route} is deferred but not documented on the acceptance page")
    notes.append(
        f"{sum(map(len, advertised.values()))} advertised routes classified, "
        f"{len(deferred)} deferred documented"
    )
    return problems, notes


CHECKS = {
    "acceptance": check_acceptance,
    "boundaries": check_boundaries,
    "versions": check_versions,
    "endpoints": check_endpoints,
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "checks",
        nargs="*",
        choices=sorted(CHECKS),
        help="checks to run (default: all)",
    )
    args = parser.parse_args(argv)
    # Python 3.13+ validates a `nargs="*"` default against `choices` as a single value, so the
    # "all checks" default is applied here instead of in argparse.
    checks = args.checks or sorted(CHECKS)
    failed = False
    for name in checks:
        problems, notes = CHECKS[name]()
        for note in notes:
            print(f"{name}: {note}")
        if problems:
            failed = True
            for problem in problems:
                print(f"{name}: FAIL {problem}", file=sys.stderr)
        else:
            print(f"{name}: ok")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
