#!/usr/bin/env python3
"""Build and validate GameNode .gnplugin ZIP packages without running package code."""

from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import sys
import tempfile
import zipfile

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey


DOMAIN = b"GameNode/.gnplugin/integrity/v1\0"
BUNDLE_DOMAIN = b"GameNode/plugin-bundle/v1\0"
MAX_COMPRESSED = 256 * 1024 * 1024
MAX_EXPANDED = 512 * 1024 * 1024
MAX_ENTRIES = 4096
MAX_METADATA = 4 * 1024 * 1024
MAX_BUNDLE_COMPRESSED = 512 * 1024 * 1024
MAX_BUNDLE_EXPANDED = 2 * 1024 * 1024 * 1024
MAX_BUNDLE_COMPONENTS = 8
INTEGRITY = "integrity.json"
SIGNATURE = "signature.ed25519"
DEV_NOTICE = "DEVELOPMENT-KEY.txt"
BUNDLE_MANIFEST = "bundle.json"


class PackageError(Exception):
    pass


def _object_pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise PackageError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _safe_name(name: str) -> str:
    if not name or "\\" in name or "\0" in name:
        raise PackageError(f"unsafe package path: {name!r}")
    path = PurePosixPath(name)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        raise PackageError(f"unsafe package path: {name!r}")
    if path.parts and (":" in path.parts[0] or path.parts[0].startswith("//")):
        raise PackageError(f"unsafe package path: {name!r}")
    normalized = path.as_posix()
    if normalized != name or normalized.endswith("/"):
        raise PackageError(f"non-canonical package path: {name!r}")
    return normalized


PLATFORM_ID = re.compile(r"^[a-z0-9._-]{1,64}$")


def _validate_review_fields(manifest: dict) -> None:
    """Validate the optional review fields shared with the Rust manifest parser."""
    platforms = manifest.get("supported_platforms")
    if platforms is not None:
        if not isinstance(platforms, list) or len(platforms) > 16:
            raise PackageError("supported_platforms must be a list of at most 16 values")
        seen: set[str] = set()
        for platform in platforms:
            if (
                not isinstance(platform, str)
                or not PLATFORM_ID.fullmatch(platform)
                or platform in seen
            ):
                raise PackageError(
                    "supported_platforms must contain unique lowercase identifiers"
                )
            seen.add(platform)
    permissions = manifest.get("permissions")
    if permissions is not None:
        if not isinstance(permissions, list):
            raise PackageError("permissions must be a list")
        for permission in permissions:
            if not isinstance(permission, dict):
                raise PackageError("each permission must be an object")
            impact = permission.get("impact_if_disabled")
            if impact is not None and (
                not isinstance(impact, str)
                or not impact.strip()
                or len(impact.encode("utf-8")) > 512
                or any(ord(char) < 32 or ord(char) == 127 for char in impact)
            ):
                raise PackageError(
                    "permission impact_if_disabled must be non-empty bounded plain text"
                )


def _load_manifest(raw: bytes) -> dict:
    if len(raw) > MAX_METADATA:
        raise PackageError("plugin.json exceeds the 4 MiB metadata limit")
    try:
        manifest = json.loads(raw.decode("utf-8"), object_pairs_hook=_object_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PackageError(f"invalid plugin.json: {exc}") from exc
    if not isinstance(manifest, dict) or manifest.get("schema") != 1:
        raise PackageError("plugin.json must be a schema-1 object")
    publisher = manifest.get("publisher")
    try:
        public_key = base64.b64decode(publisher["public_key"], validate=True)
    except (TypeError, KeyError, ValueError) as exc:
        raise PackageError("publisher.public_key must be base64 Ed25519 key bytes") from exc
    if len(public_key) != 32:
        raise PackageError("publisher.public_key must decode to exactly 32 bytes")
    targets = manifest.get("targets")
    if not isinstance(targets, list) or not targets:
        raise PackageError("plugin.json must declare at least one target")
    license_info = manifest.get("license")
    if not isinstance(license_info, dict) or not license_info.get("files"):
        raise PackageError("plugin.json must declare local license files")
    _validate_review_fields(manifest)
    return manifest


def _read_source(root: Path) -> dict[str, bytes]:
    root = root.resolve(strict=True)
    if not root.is_dir():
        raise PackageError("package source must be a directory")
    files: dict[str, bytes] = {}
    folded: dict[str, str] = {}
    total = 0
    for path in sorted(root.rglob("*")):
        mode = path.lstat().st_mode
        if stat.S_ISLNK(mode) or not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
            raise PackageError(f"links and special files are not allowed: {path}")
        if not stat.S_ISREG(mode):
            continue
        relative = _safe_name(path.relative_to(root).as_posix())
        key = relative.casefold()
        if key in folded:
            raise PackageError(f"case-colliding paths: {folded[key]} and {relative}")
        folded[key] = relative
        data = path.read_bytes()
        total += len(data)
        if total > MAX_EXPANDED:
            raise PackageError("package payload exceeds the 512 MiB expansion limit")
        files[relative] = data
    if len(files) > MAX_ENTRIES:
        raise PackageError("package contains more than 4096 files")
    if "plugin.json" not in files:
        raise PackageError("package source is missing plugin.json")
    manifest = _load_manifest(files["plugin.json"])
    referenced = list(manifest["license"]["files"])
    referenced.extend(manifest["license"].get("notices", []))
    referenced.extend(manifest.get("resources", {}).values())
    referenced.extend(schema["asset"] for schema in manifest.get("schemas", []))
    referenced.extend(target["entrypoint"] for target in manifest["targets"])
    for name in referenced:
        normalized = _safe_name(name)
        if normalized not in files:
            raise PackageError(f"manifest references missing package file: {normalized}")
    return files


def _load_private_key(path: Path) -> Ed25519PrivateKey:
    try:
        key = serialization.load_pem_private_key(path.read_bytes(), password=None)
    except (ValueError, TypeError) as exc:
        raise PackageError("signing key must be an unencrypted PEM Ed25519 private key") from exc
    if not isinstance(key, Ed25519PrivateKey):
        raise PackageError("signing key is not Ed25519")
    return key


def _signed_files(files: dict[str, bytes], key: Ed25519PrivateKey, development: bool) -> dict[str, bytes]:
    files = dict(files)
    manifest = _load_manifest(files["plugin.json"])
    public_key = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    declared_key = base64.b64decode(manifest["publisher"]["public_key"], validate=True)
    if public_key != declared_key:
        raise PackageError("signing private key does not match plugin.json publisher.public_key")
    if INTEGRITY in files or SIGNATURE in files or DEV_NOTICE in files:
        raise PackageError("source must not contain generated integrity/signature/development files")
    if development:
        files[DEV_NOTICE] = (
            b"DEVELOPMENT KEY ONLY\n"
            b"This package was signed with a locally generated test key.\n"
            b"Do not distribute it as a production publisher package.\n"
        )
    hashes = {name: hashlib.sha256(data).hexdigest() for name, data in sorted(files.items())}
    raw_integrity = json.dumps(
        {"schema": 1, "files": hashes}, ensure_ascii=False, separators=(",", ":")
    ).encode("utf-8")
    if len(raw_integrity) > MAX_METADATA:
        raise PackageError("integrity.json exceeds the 4 MiB metadata limit")
    files[INTEGRITY] = raw_integrity
    files[SIGNATURE] = key.sign(DOMAIN + raw_integrity)
    return files


def _verify_files(files: dict[str, bytes]) -> None:
    if INTEGRITY not in files or SIGNATURE not in files or "plugin.json" not in files:
        raise PackageError("package is missing plugin.json or integrity metadata")
    manifest = _load_manifest(files["plugin.json"])
    try:
        integrity = json.loads(files[INTEGRITY].decode("utf-8"), object_pairs_hook=_object_pairs)
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PackageError(f"invalid integrity.json: {exc}") from exc
    if not isinstance(integrity, dict) or set(integrity) != {"schema", "files"} or integrity["schema"] != 1:
        raise PackageError("integrity.json must be a schema-1 object with files only")
    declared = integrity["files"]
    if not isinstance(declared, dict) or not declared:
        raise PackageError("integrity.json files must be a non-empty object")
    payload = set(files) - {INTEGRITY, SIGNATURE}
    if payload != set(declared):
        raise PackageError("integrity.json does not list the exact package payload")
    for name, data in files.items():
        if name in (INTEGRITY, SIGNATURE):
            continue
        _safe_name(name)
        digest = declared.get(name)
        if not isinstance(digest, str) or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise PackageError(f"invalid SHA-256 digest for {name}")
        if hashlib.sha256(data).hexdigest() != digest:
            raise PackageError(f"payload hash mismatch: {name}")
    if len(files[SIGNATURE]) != 64:
        raise PackageError("signature.ed25519 must contain 64 raw bytes")
    public_key = base64.b64decode(manifest["publisher"]["public_key"], validate=True)
    try:
        Ed25519PublicKey.from_public_bytes(public_key).verify(files[SIGNATURE], DOMAIN + files[INTEGRITY])
    except Exception as exc:
        raise PackageError("invalid publisher integrity signature") from exc


def _archive_files(archive: zipfile.ZipFile) -> dict[str, bytes]:
    if len(archive.infolist()) > MAX_ENTRIES:
        raise PackageError("package contains more than 4096 entries")
    result: dict[str, bytes] = {}
    folded: dict[str, str] = {}
    expanded = 0
    metadata = 0
    for info in archive.infolist():
        raw_name = info.filename[:-1] if info.is_dir() else info.filename
        name = _safe_name(raw_name)
        mode = (info.external_attr >> 16) & 0xFFFF
        kind = stat.S_IFMT(mode)
        expected_kind = stat.S_IFDIR if info.is_dir() else stat.S_IFREG
        if kind not in (0, expected_kind):
            raise PackageError(f"links or special archive entry rejected: {name}")
        if name.casefold() in folded:
            raise PackageError(f"duplicate or case-colliding archive path: {name}")
        folded[name.casefold()] = name
        if info.is_dir():
            continue
        if name in result:
            raise PackageError(f"duplicate archive path: {name}")
        expanded += info.file_size
        if expanded > MAX_EXPANDED:
            raise PackageError("package expansion exceeds 512 MiB")
        if name in ("plugin.json", INTEGRITY, SIGNATURE) or name.startswith("licenses/"):
            metadata += info.file_size
            if metadata > MAX_METADATA:
                raise PackageError("package metadata exceeds 4 MiB")
        with archive.open(info, "r") as source:
            data = source.read(MAX_EXPANDED + 1)
        if len(data) != info.file_size:
            raise PackageError(f"truncated or inconsistent archive entry: {name}")
        result[name] = data
    return result


def pack(source: Path, output: Path, key_path: Path) -> None:
    files = _read_source(source)
    development = Path(str(key_path) + ".development-key").exists()
    signed = _signed_files(files, _load_private_key(key_path), development)
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data in sorted(signed.items()):
            info = zipfile.ZipInfo(name)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            archive.writestr(info, data)
    if output.stat().st_size > MAX_COMPRESSED:
        output.unlink(missing_ok=True)
        raise PackageError("package archive exceeds 256 MiB compressed limit")
    verify_archive(output)


def verify_archive(path: Path) -> None:
    if path.stat().st_size > MAX_COMPRESSED:
        raise PackageError("package archive exceeds 256 MiB compressed limit")
    try:
        with zipfile.ZipFile(path) as archive:
            _verify_files(_archive_files(archive))
    except zipfile.BadZipFile as exc:
        raise PackageError("invalid ZIP package") from exc


def _component_manifest(path: Path) -> dict:
    if not path.is_file():
        raise PackageError(f"missing component package: {path}")
    try:
        with zipfile.ZipFile(path) as archive:
            files = _archive_files(archive)
    except zipfile.BadZipFile as exc:
        raise PackageError(f"invalid component ZIP: {path}") from exc
    _verify_files(files)
    return _load_manifest(files["plugin.json"])


def _stream_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _valid_identifier(value: object) -> bool:
    return isinstance(value, str) and 0 < len(value) <= 128 and all(
        character.isascii() and (character.isalnum() or character in "._-")
        for character in value
    )


def _valid_component_path(path: object) -> bool:
    if not isinstance(path, str) or not path.startswith("components/") or not path.endswith(".gnplugin"):
        return False
    if "\\" in path or any(ord(character) < 0x20 for character in path):
        return False
    return all(part and part not in (".", "..") and ":" not in part for part in path.split("/"))


def _bundle_manifest(components: list[Path], key: Ed25519PrivateKey, bundle_id: str, name: str, version: str) -> tuple[dict, list[tuple[Path, str]]]:
    if not _valid_identifier(bundle_id):
        raise PackageError("bundle id must be 1-128 ASCII letters, digits, '.', '_' or '-'")
    if not name.strip() or len(name) > 200 or any(ord(character) < 0x20 for character in name):
        raise PackageError("bundle name must be non-empty plain text up to 200 characters")
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", version):
        raise PackageError("bundle version must be a SemVer string")
    if not 1 <= len(components) <= MAX_BUNDLE_COMPONENTS:
        raise PackageError(f"a bundle needs 1 to {MAX_BUNDLE_COMPONENTS} components")
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    key_id = None
    plugin_ids: set[str] = set()
    paths: set[str] = set()
    entries: list[dict] = []
    declared: list[tuple[Path, str]] = []
    for component in components:
        manifest = _component_manifest(component)
        publisher = manifest["publisher"]
        component_public = base64.b64decode(publisher["public_key"], validate=True)
        if component_public != public:
            raise PackageError(
                f"component {component.name} must be signed by the bundle publisher key"
            )
        component_key_id = publisher.get("key_id")
        if not isinstance(component_key_id, str) or not component_key_id:
            raise PackageError(f"component {component.name} has no publisher key id")
        if key_id is None:
            key_id = component_key_id
        elif key_id != component_key_id:
            raise PackageError("bundle components must share one publisher key id")
        plugin_id = manifest.get("id")
        kind = manifest.get("kind")
        if not _valid_identifier(plugin_id):
            raise PackageError(f"component {component.name} has an invalid plugin id")
        if kind not in ("provider", "extension"):
            raise PackageError(f"component {component.name} must be a provider or extension package")
        if plugin_id in plugin_ids:
            raise PackageError(f"bundle repeats plugin id {plugin_id}")
        if component.suffix != ".gnplugin":
            raise PackageError(f"component {component.name} must be a .gnplugin archive")
        archive_path = _safe_name(f"components/{component.name}")
        if archive_path.casefold() in paths:
            raise PackageError(f"bundle repeats component path {archive_path}")
        plugin_ids.add(plugin_id)
        paths.add(archive_path.casefold())
        entries.append(
            {
                "path": archive_path,
                "sha256": _stream_digest(component),
                "plugin_id": plugin_id,
                "kind": kind,
            }
        )
        declared.append((component, archive_path))
    entries.sort(key=lambda entry: entry["plugin_id"])
    declared.sort(key=lambda entry: entry[1])
    manifest = {
        "schema": 1,
        "id": bundle_id,
        "name": name,
        "version": version,
        "publisher": {
            "key_id": key_id,
            "public_key": base64.b64encode(public).decode("ascii"),
        },
        "components": entries,
    }
    return manifest, declared


def _bundle_archive_files(path: Path) -> dict[str, bytes]:
    if path.stat().st_size > MAX_BUNDLE_COMPRESSED:
        raise PackageError("bundle archive exceeds the 512 MiB compressed limit")
    try:
        with zipfile.ZipFile(path) as archive:
            infos = archive.infolist()
            if len(infos) > MAX_BUNDLE_COMPONENTS + 3:
                raise PackageError("bundle contains too many entries")
            files: dict[str, bytes] = {}
            folded: set[str] = set()
            expanded = 0
            for info in infos:
                name = _safe_name(info.filename[:-1] if info.is_dir() else info.filename)
                if info.is_dir():
                    raise PackageError(f"bundle must not contain directory entries: {name}")
                mode = (info.external_attr >> 16) & 0xFFFF
                if stat.S_IFMT(mode) not in (0, stat.S_IFREG):
                    raise PackageError(f"bundle entry is not a regular file: {name}")
                if name.casefold() in folded:
                    raise PackageError(f"duplicate or case-colliding bundle path: {name}")
                folded.add(name.casefold())
                allowed = (
                    name in (BUNDLE_MANIFEST, SIGNATURE, DEV_NOTICE)
                    or (name.startswith("components/") and name.endswith(".gnplugin"))
                )
                if not allowed:
                    raise PackageError(f"unexpected bundle entry: {name}")
                expanded += info.file_size
                if expanded > MAX_BUNDLE_EXPANDED:
                    raise PackageError("bundle expansion exceeds 2 GiB")
                files[name] = archive.read(info)
    except zipfile.BadZipFile as exc:
        raise PackageError("invalid ZIP bundle") from exc
    return files


def _validate_bundle_manifest(manifest: object) -> dict:
    if not isinstance(manifest, dict) or set(manifest) != {
        "schema",
        "id",
        "name",
        "version",
        "publisher",
        "components",
    }:
        raise PackageError("bundle.json must contain schema, id, name, version, publisher and components")
    if manifest["schema"] != 1:
        raise PackageError("bundle.json must be schema 1")
    if not _valid_identifier(manifest["id"]):
        raise PackageError("bundle.json has an invalid id")
    if not isinstance(manifest["name"], str) or not manifest["name"].strip() or len(manifest["name"]) > 200:
        raise PackageError("bundle.json has an invalid name")
    if not isinstance(manifest["version"], str) or not re.fullmatch(
        r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", manifest["version"]
    ):
        raise PackageError("bundle.json has an invalid SemVer version")
    publisher = manifest["publisher"]
    if not isinstance(publisher, dict) or set(publisher) != {"key_id", "public_key"}:
        raise PackageError("bundle.json has an invalid publisher")
    if not isinstance(publisher["key_id"], str) or not publisher["key_id"]:
        raise PackageError("bundle.json has no publisher key id")
    try:
        public = base64.b64decode(publisher["public_key"], validate=True)
    except (TypeError, ValueError) as exc:
        raise PackageError("bundle.json publisher.public_key is not base64") from exc
    if len(public) != 32:
        raise PackageError("bundle.json publisher.public_key must decode to 32 bytes")
    components = manifest["components"]
    if not isinstance(components, list) or not 1 <= len(components) <= MAX_BUNDLE_COMPONENTS:
        raise PackageError(f"bundle.json needs 1 to {MAX_BUNDLE_COMPONENTS} components")
    plugin_ids: set[str] = set()
    paths: set[str] = set()
    for component in components:
        if not isinstance(component, dict) or set(component) != {"path", "sha256", "plugin_id", "kind"}:
            raise PackageError("bundle.json has an invalid component")
        if not _valid_component_path(component["path"]):
            raise PackageError(f"unsafe component path: {component['path']!r}")
        digest = component["sha256"]
        if not isinstance(digest, str) or len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            raise PackageError(f"invalid component SHA-256: {component['path']}")
        if not _valid_identifier(component["plugin_id"]):
            raise PackageError("bundle.json has an invalid component plugin id")
        if component["kind"] not in ("provider", "extension"):
            raise PackageError("bundle.json component kind must be provider or extension")
        if component["plugin_id"] in plugin_ids or component["path"].casefold() in paths:
            raise PackageError("bundle.json repeats a component id or path")
        plugin_ids.add(component["plugin_id"])
        paths.add(component["path"].casefold())
    return manifest


def bundle(components: list[Path], bundle_id: str, name: str, version: str, output: Path, key_path: Path) -> None:
    key = _load_private_key(key_path)
    development = Path(str(key_path) + ".development-key").exists()
    manifest, declared = _bundle_manifest(components, key, bundle_id, name, version)
    raw_manifest = json.dumps(manifest, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8")
    signature = key.sign(BUNDLE_DOMAIN + raw_manifest)
    output.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(output, "w") as archive:
        info = zipfile.ZipInfo(BUNDLE_MANIFEST, (1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = (stat.S_IFREG | 0o644) << 16
        archive.writestr(info, raw_manifest)
        for component, archive_path in declared:
            info = zipfile.ZipInfo(archive_path, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            with archive.open(info, "w") as destination, component.open("rb") as source:
                shutil.copyfileobj(source, destination, 1024 * 1024)
        info = zipfile.ZipInfo(SIGNATURE, (1980, 1, 1, 0, 0, 0))
        info.compress_type = zipfile.ZIP_DEFLATED
        info.external_attr = (stat.S_IFREG | 0o644) << 16
        archive.writestr(info, signature)
        if development:
            info = zipfile.ZipInfo(DEV_NOTICE, (1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            archive.writestr(
                info,
                b"DEVELOPMENT KEY ONLY\nThis bundle was signed with a locally generated test key.\n",
            )
    if output.stat().st_size > MAX_BUNDLE_COMPRESSED:
        output.unlink(missing_ok=True)
        raise PackageError("bundle archive exceeds the 512 MiB compressed limit")
    bundle_verify(output)


def bundle_verify(path: Path) -> None:
    files = _bundle_archive_files(path)
    if BUNDLE_MANIFEST not in files or SIGNATURE not in files:
        raise PackageError("bundle is missing bundle.json or signature.ed25519")
    raw_manifest = files[BUNDLE_MANIFEST]
    if len(raw_manifest) > MAX_METADATA:
        raise PackageError("bundle.json exceeds the 4 MiB metadata limit")
    try:
        manifest = _validate_bundle_manifest(
            json.loads(raw_manifest.decode("utf-8"), object_pairs_hook=_object_pairs)
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise PackageError(f"invalid bundle.json: {exc}") from exc
    signature = files[SIGNATURE]
    if len(signature) != 64:
        raise PackageError("bundle signature.ed25519 must contain 64 raw bytes")
    public = base64.b64decode(manifest["publisher"]["public_key"], validate=True)
    try:
        Ed25519PublicKey.from_public_bytes(public).verify(signature, BUNDLE_DOMAIN + raw_manifest)
    except Exception as exc:
        raise PackageError("invalid bundle signature") from exc
    declared = {component["path"]: component for component in manifest["components"]}
    expected = set(declared) | {BUNDLE_MANIFEST, SIGNATURE}
    if DEV_NOTICE in files:
        expected.add(DEV_NOTICE)
    if set(files) != expected:
        raise PackageError("bundle contains entries not listed in bundle.json")
    for path_name, component in declared.items():
        data = files[path_name]
        if hashlib.sha256(data).hexdigest() != component["sha256"]:
            raise PackageError(f"bundle component hash mismatch: {path_name}")
        try:
            with zipfile.ZipFile(io.BytesIO(data)) as inner:
                inner_files = _archive_files(inner)
        except zipfile.BadZipFile as exc:
            raise PackageError(f"invalid component ZIP: {path_name}") from exc
        _verify_files(inner_files)
        inner_manifest = _load_manifest(inner_files["plugin.json"])
        if inner_manifest["publisher"]["public_key"] != manifest["publisher"]["public_key"]:
            raise PackageError(f"component publisher differs from the bundle publisher: {path_name}")
        if inner_manifest.get("id") != component["plugin_id"]:
            raise PackageError(f"component plugin id mismatch: {path_name}")
        if inner_manifest.get("kind") != component["kind"]:
            raise PackageError(f"component kind mismatch: {path_name}")


def keygen(private_path: Path) -> None:
    private_path.parent.mkdir(parents=True, exist_ok=True)
    if private_path.exists():
        raise PackageError(f"refusing to overwrite existing key: {private_path}")
    key = Ed25519PrivateKey.generate()
    private_path.write_bytes(key.private_bytes(
        serialization.Encoding.PEM,
        serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption(),
    ))
    try:
        os.chmod(private_path, 0o600)
    except OSError:
        pass
    marker = Path(str(private_path) + ".development-key")
    marker.write_text("DEVELOPMENT ONLY: generated for local fixtures; not a production publisher key.\n", encoding="utf-8")
    public = key.public_key().public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)
    print("WARNING: DEVELOPMENT KEY ONLY; never use this key for release packages.")
    print("publisher.public_key (base64): " + base64.b64encode(public).decode("ascii"))
    print("key fingerprint: SHA256:" + hashlib.sha256(public).hexdigest())


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    gen = commands.add_parser("keygen", help="generate a clearly marked development key")
    gen.add_argument("--development", action="store_true", required=True, help="required acknowledgement; key is test-only")
    gen.add_argument("--private-key", type=Path, required=True)
    build = commands.add_parser("pack", help="sign and create a .gnplugin ZIP package")
    build.add_argument("--source", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--private-key", type=Path, required=True)
    check = commands.add_parser("verify", help="validate archive paths, hashes, and signature without executing code")
    check.add_argument("package", type=Path)
    bundle_command = commands.add_parser("bundle", help="sign a .gnbundle containing .gnplugin components")
    bundle_command.add_argument("--component", type=Path, action="append", required=True, help="repeat for each component package")
    bundle_command.add_argument("--id", required=True)
    bundle_command.add_argument("--name", required=True)
    bundle_command.add_argument("--version", required=True)
    bundle_command.add_argument("--output", type=Path, required=True)
    bundle_command.add_argument("--private-key", type=Path, required=True)
    bundle_check = commands.add_parser("bundle-verify", help="validate a .gnbundle and every component without executing code")
    bundle_check.add_argument("bundle", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command == "keygen":
            keygen(args.private_key)
        elif args.command == "pack":
            pack(args.source, args.output, args.private_key)
            print(f"verified package: {args.output}")
        elif args.command == "bundle":
            bundle(
                args.component,
                args.id,
                args.name,
                args.version,
                args.output,
                args.private_key,
            )
            print(f"verified bundle: {args.output}")
        elif args.command == "bundle-verify":
            bundle_verify(args.bundle)
            print(f"valid bundle: {args.bundle}")
        else:
            verify_archive(args.package)
            print(f"valid package: {args.package}")
        return 0
    except (PackageError, OSError) as exc:
        print(f"gnplugin: error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
