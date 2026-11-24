import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile


SCRIPT = Path(__file__).resolve().parents[1] / "plugin-package.py"
spec = importlib.util.spec_from_file_location("plugin_package_tool_bundle", SCRIPT)
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)


class PluginBundleToolTests(unittest.TestCase):
    def _package(self, base: Path, name: str, kind: str, plugin_id: str, key_path: Path) -> Path:
        source = base / f"{name}-source"
        (source / "licenses").mkdir(parents=True)
        (source / "bin").mkdir()
        key = tool._load_private_key(key_path)
        public = key.public_key().public_bytes(
            tool.serialization.Encoding.Raw,
            tool.serialization.PublicFormat.Raw,
        )
        manifest = {
            "schema": 1,
            "id": plugin_id,
            "kind": kind,
            "name": name,
            "version": "0.1.0",
            "api": {"min": "1.6.0", "max": "1.6.0"},
            "permissions": [],
            "targets": [{"os": "linux", "arch": "x86_64", "abi": "gnu", "entrypoint": "bin/plugin"}],
            "publisher": {
                "key_id": "bundle-fixture",
                "public_key": tool.base64.b64encode(public).decode("ascii"),
            },
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]},
        }
        if kind == "provider":
            manifest["provider_id"] = "bundle-provider"
        (source / "plugin.json").write_text(json.dumps(manifest), encoding="utf-8")
        (source / "licenses/LICENSE").write_text("fixture license\n", encoding="utf-8")
        (source / "bin/plugin").write_bytes(b"fixture executable\n")
        package = base / f"{name}.gnplugin"
        tool.pack(source, package, key_path)
        return package

    def _fixture(self, base: Path) -> tuple[Path, Path, Path]:
        key_path = base / "dev.pem"
        tool.keygen(key_path)
        provider = self._package(base, "provider", "provider", "org.example.bundle.provider", key_path)
        extension = self._package(base, "extension", "extension", "org.example.bundle.launch", key_path)
        return key_path, provider, extension

    def test_round_trip_writes_a_verifiable_bundle_with_component_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            key_path, provider, extension = self._fixture(base)
            output = base / "example.gnbundle"
            tool.bundle(
                [provider, extension],
                "org.example.bundle",
                "Bundle Fixture",
                "0.1.0",
                output,
                key_path,
            )
            tool.bundle_verify(output)
            with zipfile.ZipFile(output) as archive:
                manifest = json.loads(archive.read("bundle.json"))
                names = set(archive.namelist())
            self.assertEqual(manifest["id"], "org.example.bundle")
            self.assertEqual(manifest["version"], "0.1.0")
            self.assertEqual(len(manifest["components"]), 2)
            self.assertEqual(
                {component["kind"] for component in manifest["components"]},
                {"provider", "extension"},
            )
            self.assertIn("components/provider.gnplugin", names)
            self.assertIn("components/extension.gnplugin", names)
            self.assertIn("signature.ed25519", names)
            self.assertIn("DEVELOPMENT-KEY.txt", names)

    def test_a_modified_component_or_missing_entry_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            key_path, provider, extension = self._fixture(base)
            output = base / "example.gnbundle"
            tool.bundle([provider, extension], "org.example.bundle", "Bundle", "0.1.0", output, key_path)

            tampered = base / "tampered.gnbundle"
            with zipfile.ZipFile(output) as source, zipfile.ZipFile(tampered, "w") as target:
                for info in source.infolist():
                    data = source.read(info)
                    if info.filename == "components/provider.gnplugin":
                        data = data[:-1] + bytes([data[-1] ^ 1])
                    target.writestr(info, data)
            with self.assertRaisesRegex(tool.PackageError, "component hash mismatch"):
                tool.bundle_verify(tampered)

            missing = base / "missing.gnbundle"
            with zipfile.ZipFile(output) as source, zipfile.ZipFile(missing, "w") as target:
                for info in source.infolist():
                    if info.filename != "components/extension.gnplugin":
                        target.writestr(info, source.read(info))
            with self.assertRaisesRegex(tool.PackageError, "not listed"):
                tool.bundle_verify(missing)

    def test_mixed_publishers_and_duplicate_ids_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            key_path, provider, _ = self._fixture(base)
            other_key = base / "other.pem"
            tool.keygen(other_key)
            other = self._package(base, "other", "extension", "org.example.bundle.other", other_key)
            with self.assertRaisesRegex(tool.PackageError, "bundle publisher key"):
                tool.bundle([provider, other], "org.example.bundle", "Bundle", "0.1.0", base / "a.gnbundle", key_path)

            duplicate = self._package(base, "duplicate", "provider", "org.example.bundle.provider", key_path)
            with self.assertRaisesRegex(tool.PackageError, "repeats plugin id"):
                tool.bundle([provider, duplicate], "org.example.bundle", "Bundle", "0.1.0", base / "b.gnbundle", key_path)

    def test_an_unexpected_entry_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            key_path, provider, _ = self._fixture(base)
            output = base / "example.gnbundle"
            tool.bundle([provider], "org.example.bundle", "Bundle", "0.1.0", output, key_path)
            extra = base / "extra.gnbundle"
            with zipfile.ZipFile(output) as source, zipfile.ZipFile(extra, "w") as target:
                for info in source.infolist():
                    target.writestr(info, source.read(info))
                target.writestr("components/nested.gnbundle", b"not a package")
            with self.assertRaisesRegex(tool.PackageError, "unexpected bundle entry"):
                tool.bundle_verify(extra)

    def test_expansion_and_compressed_limits_are_enforced(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            key_path, provider, extension = self._fixture(base)
            output = base / "example.gnbundle"
            tool.bundle([provider, extension], "org.example.bundle", "Bundle", "0.1.0", output, key_path)
            original_expanded = tool.MAX_BUNDLE_EXPANDED
            tool.MAX_BUNDLE_EXPANDED = 10
            try:
                with self.assertRaisesRegex(tool.PackageError, "expansion exceeds"):
                    tool.bundle_verify(output)
            finally:
                tool.MAX_BUNDLE_EXPANDED = original_expanded
            original_compressed = tool.MAX_BUNDLE_COMPRESSED
            tool.MAX_BUNDLE_COMPRESSED = 10
            try:
                with self.assertRaisesRegex(tool.PackageError, "compressed limit"):
                    tool.bundle_verify(output)
            finally:
                tool.MAX_BUNDLE_COMPRESSED = original_compressed
            tool.bundle_verify(output)


if __name__ == "__main__":
    unittest.main()
