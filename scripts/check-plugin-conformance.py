#!/usr/bin/env python3
"""Conformance checks for the third-party plugin protocol surface and its examples.

These checks are intentionally independent of the Rust build: they can be run before
committing a protocol or example change. They cover:

* generated Go bindings freshness against the canonical ``plugin.proto``;
* public dependency boundaries for the example modules (no internal Core crates);
* the API window and identities declared by the example manifests and source constants;
* the ``DescribeApi`` route allowlist parsed from the host's served-method tables.

Nothing here executes plugin code. Exit status is non-zero when a check fails.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
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
GENERATE_PROTO = REPO_ROOT / "examples/plugins/go-grpc/scripts/generate-proto.sh"

# (generated directory, Go import path of the generated package, files to compare)
GO_BINDINGS = (
    (
        REPO_ROOT / "examples/plugins/go-grpc/proto",
        "example.com/gamenode/plugins/go-grpc/proto",
        ("plugin.pb.go", "plugin_grpc.pb.go"),
    ),
    (
        REPO_ROOT / "examples/go/proto",
        "example.com/gamenode/plugin-example/proto",
        ("plugin.pb.go", "plugin_grpc.pb.go"),
    ),
)

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
GO_DEPENDENCY_DENYLIST = (
    "github.com/game-node-app/",
    "gamenode-core",
    "gamenode-platform",
)

# Go string escapes used by protoc-gen-go's concatenated raw-descriptor literal.
_GO_ESCAPES = {
    "a": 7,
    "b": 8,
    "f": 12,
    "n": 10,
    "r": 13,
    "t": 9,
    "v": 11,
    "\\": 92,
    '"': 34,
    "'": 39,
}


class Problems(list[str]):
    """A check's failures."""

    def fail(self, message: str) -> None:
        self.append(message)


def read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


# ---------------------------------------------------------------------------
# Protobuf and generated bindings
# ---------------------------------------------------------------------------


def decode_go_string(segment: str) -> bytes:
    """Decode the escapes protoc-gen-go emits inside a concatenated Go string literal."""
    out = bytearray()
    index = 0
    while index < len(segment):
        char = segment[index]
        if char != "\\":
            out.extend(char.encode("utf-8"))
            index += 1
            continue
        index += 1
        if index >= len(segment):
            raise ValueError("dangling escape in generated Go string")
        escape = segment[index]
        if escape == "x":
            out.append(int(segment[index + 1 : index + 3], 16))
            index += 3
        elif escape in _GO_ESCAPES:
            out.append(_GO_ESCAPES[escape])
            index += 1
        else:
            raise ValueError(f"unsupported Go escape \\{escape}")
    return bytes(out)


def go_raw_descriptor(source: str) -> bytes:
    """The FileDescriptorProto embedded by protoc-gen-go for ``plugin.proto``."""
    match = re.search(
        r'const file_plugin_proto_rawDesc = ((?:"(?:[^"\\]|\\.)*"\s*\+?\s*)+)',
        source,
    )
    if not match:
        raise ValueError("file_plugin_proto_rawDesc is missing")
    segments = re.findall(r'"((?:[^"\\]|\\.)*)"', match.group(1))
    return b"".join(decode_go_string(segment) for segment in segments)


def _varint(data: bytes, index: int) -> tuple[int, int]:
    value = 0
    shift = 0
    while True:
        byte = data[index]
        index += 1
        value |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            return value, index


def descriptor_set_files(data: bytes) -> list[bytes]:
    """Split a serialized FileDescriptorSet into its FileDescriptorProto payloads."""
    files: list[bytes] = []
    index = 0
    while index < len(data):
        key, index = _varint(data, index)
        field, wire = key >> 3, key & 7
        if wire != 2:
            raise ValueError(f"unexpected FileDescriptorSet wire type {wire}")
        length, index = _varint(data, index)
        payload = data[index : index + length]
        if len(payload) != length:
            raise ValueError("truncated FileDescriptorSet")
        index += length
        if field == 1:
            files.append(payload)
    return files


def protoc_include_dir(protoc: str) -> Path | None:
    """A directory that contains ``google/protobuf/struct.proto``."""
    candidates = [
        Path(value)
        for value in (
            os.environ.get("PROTOC_INCLUDE_DIR", ""),
            "/usr/include",
            "/usr/local/include",
            str(Path(protoc).resolve().parent / "include"),
        )
        if value
    ]
    for candidate in candidates:
        if (candidate / "google/protobuf/struct.proto").is_file():
            return candidate
    return None


def compile_descriptor(protoc: str) -> bytes:
    include = protoc_include_dir(protoc)
    if include is None:
        raise RuntimeError(
            "set PROTOC_INCLUDE_DIR to a directory containing google/protobuf/struct.proto"
        )
    with tempfile.TemporaryDirectory(prefix="gamenode-conformance-") as temp:
        output = Path(temp) / "plugin.desc"
        result = subprocess.run(
            [
                protoc,
                f"-I{PROTO.parent}",
                f"-I{include}",
                f"--descriptor_set_out={output}",
                PROTO.name,
            ],
            cwd=PROTO.parent,
            capture_output=True,
            text=True,
        )
        if result.returncode != 0:
            raise RuntimeError(f"protoc failed: {result.stderr.strip()}")
        entries = descriptor_set_files(output.read_bytes())
    if len(entries) != 1:
        raise RuntimeError(f"expected one descriptor for plugin.proto, got {len(entries)}")
    return entries[0]


def first_difference(left: bytes, right: bytes) -> str:
    for index, (one, two) in enumerate(zip(left, right)):
        if one != two:
            return f"first difference at byte {index} (bindings {one:#04x}, proto {two:#04x})"
    return f"length differs: bindings {len(left)} bytes, proto {len(right)} bytes"


def generated_versions(source: str) -> tuple[str | None, str | None]:
    proto = re.search(r"protoc-gen-go v([0-9.]+)", source)
    grpc = re.search(r"protoc-gen-go-grpc v([0-9.]+)", source)
    return (
        proto.group(1) if proto else None,
        grpc.group(1) if grpc else None,
    )


def pinned_generator_versions() -> tuple[str, str]:
    source = read(GENERATE_PROTO)
    proto = re.search(r"^GO_PROTO_VERSION=([0-9.]+)$", source, re.M)
    grpc = re.search(r"^GO_GRPC_VERSION=([0-9.]+)$", source, re.M)
    if not proto or not grpc:
        raise RuntimeError("generate-proto.sh does not pin both generator versions")
    return proto.group(1), grpc.group(1)


def check_bindings(protoc: str | None) -> tuple[Problems, list[str]]:
    problems = Problems()
    notes: list[str] = []
    expected_proto, expected_grpc = pinned_generator_versions()
    canonical: bytes | None = None
    if protoc is None:
        problems.fail(
            "protoc is not available; install libprotoc 36.1 to check binding freshness"
        )
    else:
        try:
            canonical = compile_descriptor(protoc)
        except (RuntimeError, ValueError) as error:
            problems.fail(str(error))

    go_proto = shutil.which("protoc-gen-go")
    go_grpc = shutil.which("protoc-gen-go-grpc")
    include = protoc_include_dir(protoc) if protoc else None
    for directory, import_path, files in GO_BINDINGS:
        relative = directory.relative_to(REPO_ROOT)
        for name in files:
            path = directory / name
            if not path.is_file():
                problems.fail(f"{relative}/{name} is missing")
                continue
            source = read(path)
            proto_version, grpc_version = generated_versions(source)
            if name == "plugin.pb.go":
                if proto_version != expected_proto:
                    problems.fail(
                        f"{relative}/{name} was generated with protoc-gen-go "
                        f"{proto_version or 'an unknown version'}, expected {expected_proto}"
                    )
                if canonical is not None:
                    try:
                        embedded = go_raw_descriptor(source)
                    except ValueError as error:
                        problems.fail(f"{relative}/{name}: {error}")
                    else:
                        if embedded != canonical:
                            problems.fail(
                                f"{relative}/{name} is stale against {PROTO.relative_to(REPO_ROOT)}: "
                                f"{first_difference(embedded, canonical)}"
                            )
            elif grpc_version != expected_grpc:
                problems.fail(
                    f"{relative}/{name} was generated with protoc-gen-go-grpc "
                    f"{grpc_version or 'an unknown version'}, expected {expected_grpc}"
                )

        # When both generators exist, a full regeneration is the strongest check: it also
        # covers formatting and service stubs whose edits do not change the descriptor.
        if protoc is None or include is None or go_proto is None or go_grpc is None:
            notes.append(
                f"{relative}: full regeneration skipped "
                "(protoc, protoc-gen-go and protoc-gen-go-grpc are required)"
            )
            continue
        with tempfile.TemporaryDirectory(prefix="gamenode-bindings-") as temp:
            output = Path(temp)
            result = subprocess.run(
                [
                    protoc,
                    f"-I{PROTO.parent}",
                    f"-I{include}",
                    f"--plugin=protoc-gen-go={go_proto}",
                    f"--plugin=protoc-gen-go-grpc={go_grpc}",
                    f"--go_out={output}",
                    "--go_opt=paths=source_relative",
                    f"--go_opt=M{PROTO.name}={import_path}",
                    f"--go-grpc_out={output}",
                    "--go-grpc_opt=paths=source_relative",
                    f"--go-grpc_opt=M{PROTO.name}={import_path}",
                    PROTO.name,
                ],
                cwd=PROTO.parent,
                capture_output=True,
                text=True,
            )
            if result.returncode != 0:
                problems.fail(f"{relative}: regeneration failed: {result.stderr.strip()}")
                continue
            for name in files:
                fresh = output / name
                if not fresh.is_file():
                    problems.fail(f"{relative}: regeneration did not produce {name}")
                elif fresh.read_bytes() != (directory / name).read_bytes():
                    problems.fail(
                        f"{relative}/{name} differs from a fresh generation; "
                        "run examples/plugins/go-grpc/scripts/generate-proto.sh"
                    )
    return problems, notes


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


def go_imports(source: str) -> list[str]:
    imports: list[str] = []
    for block in re.findall(r"import\s*\((.*?)\)", source, re.S):
        for match in re.finditer(r'"([^"]+)"', block):
            imports.append(match.group(1))
    for match in re.finditer(r'import\s+"([^"]+)"', source):
        imports.append(match.group(1))
    return imports


def check_go_module(problems: Problems, root: Path) -> None:
    module = root.relative_to(REPO_ROOT)
    source = read(root / "go.mod")
    for line in source.splitlines():
        stripped = line.strip()
        if stripped.startswith("replace"):
            problems.fail(f"{module}/go.mod has a replace directive: {stripped}")
    for path in sorted(root.rglob("*.go")):
        for imported in go_imports(read(path)):
            if imported.startswith(GO_DEPENDENCY_DENYLIST):
                problems.fail(
                    f"{path.relative_to(REPO_ROOT)} imports a workspace package: {imported}"
                )


def check_boundaries() -> tuple[Problems, list[str]]:
    problems = Problems()
    check_rust_example(problems)
    check_go_module(problems, REPO_ROOT / "examples/plugins/go-grpc")
    check_go_module(problems, REPO_ROOT / "examples/go")
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

    go_source = read(REPO_ROOT / "examples/plugins/go-grpc/provider/client.go")
    go_major = re.search(r"APIMajor\s*=\s*(\d+)", go_source)
    go_minor = re.search(r"APIMinor\s*=\s*(\d+)", go_source)
    go_plugin = re.search(r'PluginID\s*=\s*"([^"]+)"', go_source)
    go_provider = re.search(r'ProviderID\s*=\s*"([^"]+)"', go_source)
    if not all((go_major, go_minor, go_plugin, go_provider)):
        problems.fail("the Go example must declare APIMajor, APIMinor, PluginID and ProviderID")
    else:
        if int(go_major.group(1)) != constants["API_MAJOR"]:
            problems.fail("the Go example API major does not match the protocol")
        if int(go_minor.group(1)) != constants["API_MINOR"]:
            problems.fail("the Go example API minor does not match the protocol")
        compare_identity(
            problems,
            REPO_ROOT / "examples/plugins/go-grpc/package/plugin.json",
            manifest(REPO_ROOT / "examples/plugins/go-grpc/package/plugin.json"),
            go_plugin.group(1),
            go_provider.group(1),
        )

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

    for relative in (
        "examples/plugins/go-grpc/package/plugin.json",
        "examples/plugins/rust-sdk/package/plugin.json",
    ):
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
    "bindings": lambda: check_bindings(shutil.which("protoc")),
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
