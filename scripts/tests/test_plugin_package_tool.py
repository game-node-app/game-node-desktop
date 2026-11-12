import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
import zipfile


SCRIPT = Path(__file__).resolve().parents[1] / "plugin-package.py"
spec = importlib.util.spec_from_file_location("plugin_package_tool", SCRIPT)
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)


class PluginPackageToolTests(unittest.TestCase):
    def _source(self, root: Path, target: str, executable: str, public_key: str) -> None:
        (root / "licenses").mkdir(parents=True)
        (root / "bin" / target).mkdir(parents=True)
        (root / "plugin.json").write_text(json.dumps({
            "schema": 1,
            "id": "org.example.fixture",
            "kind": "provider",
            "provider_id": "fixture-provider",
            "name": "Fixture Provider",
            "version": "1.0.0",
            "api": {"min": "1.4.0", "max": "1.4.0"},
            "targets": [{
                "os": target.split("-")[0],
                "arch": "x86_64",
                "abi": "gnu" if target.startswith("linux") else "msvc",
                "entrypoint": executable,
            }],
            "publisher": {"key_id": "fixture-dev", "public_key": public_key},
            "license": {"expression": "MIT", "files": ["licenses/LICENSE"]},
        }), encoding="utf-8")
        (root / "licenses/LICENSE").write_text("fixture license\n", encoding="utf-8")
        (root / executable).write_bytes(b"fixture executable bytes\n")
        # A package hook is inert data. Packing and verification must never run it.
        (root / "install.py").write_text(
            "from pathlib import Path\nPath('hook-ran').write_text('yes')\n", encoding="utf-8"
        )

    def test_linux_and_windows_fixtures_pack_and_verify_without_running_hooks(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            for target, executable in (
                ("linux-x64", "bin/linux-x64/plugin"),
                ("windows-x64", "bin/windows-x64/plugin.exe"),
            ):
                with self.subTest(target=target):
                    key_path = base / f"{target}.pem"
                    tool.keygen(key_path)
                    key = tool._load_private_key(key_path)
                    public = key.public_key().public_bytes(
                        tool.serialization.Encoding.Raw,
                        tool.serialization.PublicFormat.Raw,
                    )
                    source = base / f"source-{target}"
                    source.mkdir()
                    self._source(source, target, executable, base64.b64encode(public).decode())
                    output = base / f"fixture-{target}.gnplugin"
                    tool.pack(source, output, key_path)
                    tool.verify_archive(output)
                    self.assertFalse((source / "hook-ran").exists())
                    with zipfile.ZipFile(output) as archive:
                        names = set(archive.namelist())
                        self.assertIn("DEVELOPMENT-KEY.txt", names)
                        self.assertIn(executable, names)
                        self.assertIn("integrity.json", names)
                        self.assertIn("signature.ed25519", names)
                        self.assertIn(b"DEVELOPMENT KEY ONLY", archive.read("DEVELOPMENT-KEY.txt"))

    def test_tampered_payload_and_bad_archive_path_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            base = Path(temporary)
            key_path = base / "dev.pem"
            tool.keygen(key_path)
            key = tool._load_private_key(key_path)
            public = key.public_key().public_bytes(
                tool.serialization.Encoding.Raw, tool.serialization.PublicFormat.Raw
            )
            source = base / "source"
            source.mkdir()
            self._source(source, "linux-x64", "bin/linux-x64/plugin", base64.b64encode(public).decode())
            package = base / "fixture.gnplugin"
            tool.pack(source, package, key_path)

            tampered = base / "tampered.gnplugin"
            with zipfile.ZipFile(package) as old, zipfile.ZipFile(tampered, "w") as new:
                for name in old.namelist():
                    data = old.read(name)
                    if name == "bin/linux-x64/plugin":
                        data = b"changed bytes"
                    new.writestr(name, data)
            with self.assertRaisesRegex(tool.PackageError, "hash mismatch"):
                tool.verify_archive(tampered)

            unsafe = base / "unsafe.gnplugin"
            with zipfile.ZipFile(unsafe, "w") as archive:
                archive.writestr("../outside", b"no")
            with self.assertRaisesRegex(tool.PackageError, "unsafe package path"):
                tool.verify_archive(unsafe)


if __name__ == "__main__":
    unittest.main()
