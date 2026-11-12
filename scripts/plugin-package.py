#!/usr/bin/env python3
"""Build and validate GameNode .gnplugin ZIP packages without running package code."""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import tempfile
import zipfile

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey, Ed25519PublicKey


DOMAIN = b"GameNode/.gnplugin/integrity/v1\0"
MAX_COMPRESSED = 256 * 1024 * 1024
MAX_EXPANDED = 512 * 1024 * 1024
MAX_ENTRIES = 4096
MAX_METADATA = 4 * 1024 * 1024
INTEGRITY = "integrity.json"
SIGNATURE = "signature.ed25519"
DEV_NOTICE = "DEVELOPMENT-KEY.txt"


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
    args = parser.parse_args(argv)
    try:
        if args.command == "keygen":
            keygen(args.private_key)
        elif args.command == "pack":
            pack(args.source, args.output, args.private_key)
            print(f"verified package: {args.output}")
        else:
            verify_archive(args.package)
            print(f"valid package: {args.package}")
        return 0
    except (PackageError, OSError) as exc:
        print(f"gnplugin: error: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
