import importlib.util
import os
from pathlib import Path
import struct
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "package_check", Path(__file__).resolve().parents[1] / "verify-plugin-package.py"
)
package_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_check)


class PackageLayout(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gamenode package with spaces ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def _resources(self, *, windows):
        names = (
            ("LICENSE", "README.md", "build-manifest.json", "install.ps1", "uninstall.ps1")
            if windows else
            ("LICENSE", "README.md", "build-manifest.json", "gamenode.png", "install.sh",
             "uninstall.sh", "gamenode.sh", "desktop-integration.sh")
        )
        for name in names:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("fixture", encoding="utf-8")
        licenses = self.root / "licenses"
        licenses.mkdir(exist_ok=True)
        (licenses / "NOTICE.txt").write_text("fixture notice", encoding="utf-8")
        if windows:
            assets = self.root / "backup-assets"
            assets.mkdir()
            (assets / "build-inputs.json").write_text("{}", encoding="utf-8")

    def _windows(self):
        header = bytearray(70)
        header[:2] = b"MZ"
        struct.pack_into("<I", header, 60, 64)
        header[64:] = b"PE\x00\x00\x64\x86"
        for name in ("gamenode.exe", "gamenode-plugin-steam.exe", "gamenode-plugin-epic.exe"):
            (self.root / name).write_bytes(header)
        self._resources(windows=True)

    def _linux(self):
        directory = self.root / "libexec"
        directory.mkdir()
        header = bytearray(64)
        header[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", header, 18, 62)
        for name in ("gamenode", "gamenode-plugin-steam", "gamenode-plugin-packagekit"):
            binary = directory / name
            binary.write_bytes(header)
            binary.chmod(0o755)
        self._resources(windows=False)

    def test_windows_requires_adjacent_steam_plugin_and_keeps_paths_with_spaces(self):
        self._windows()
        self.assertEqual(package_check.verify(self.root, "windows"), [])
        (self.root / "gamenode-plugin-steam.exe").unlink()
        self.assertIn("missing regular binary", package_check.verify(self.root, "windows")[0])

    def test_windows_requires_adjacent_epic_plugin(self):
        self._windows()
        self.assertEqual(package_check.verify(self.root, "windows"), [])
        (self.root / "gamenode-plugin-epic.exe").unlink()
        errors = package_check.verify(self.root, "windows")
        self.assertEqual(len(errors), 1)
        self.assertIn("missing regular binary", errors[0])
        self.assertIn("gamenode-plugin-epic.exe", errors[0])

    def test_windows_rejects_wrong_target_and_linux_only_plugin(self):
        self._windows()
        (self.root / "gamenode-plugin-steam.exe").write_bytes(b"\x7fELF")
        (self.root / "gamenode-plugin-packagekit.exe").write_bytes(b"not supported")
        errors = package_check.verify(self.root, "windows")
        self.assertEqual(len(errors), 2)
        self.assertIn("PE binary", errors[0])
        self.assertIn("Linux package-manager", errors[1])

    @unittest.skipUnless(os.name == "posix", "Linux execute bits require POSIX stat semantics")
    def test_linux_requires_adjacent_plugins_and_user_execute_permissions(self):
        self._linux()
        self.assertEqual(package_check.verify(self.root, "linux"), [])
        plugin = self.root / "libexec/gamenode-plugin-packagekit"
        plugin.chmod(0o644)
        errors = package_check.verify(self.root, "linux")
        self.assertEqual(len(errors), 1)
        self.assertIn("not executable", errors[0])
        plugin.unlink()
        self.assertIn("missing regular binary", package_check.verify(self.root, "linux")[0])

    def test_linux_does_not_require_epic_plugin(self):
        self._linux()
        self.assertEqual(package_check.verify(self.root, "linux"), [])
        self.assertFalse((self.root / "libexec/gamenode-plugin-epic").exists())

    def test_linux_forbids_epic_plugin_binaries(self):
        self._linux()
        epic = self.root / "libexec/gamenode-plugin-epic"
        epic.write_bytes(b"\x7fELF")
        epic.chmod(0o755)
        errors = package_check.verify(self.root, "linux")
        self.assertEqual(len(errors), 1)
        self.assertIn("Windows-only Epic plugin", errors[0])
        epic.unlink()
        (self.root / "libexec/gamenode-plugin-epic.exe").write_bytes(b"MZ")
        errors = package_check.verify(self.root, "linux")
        self.assertEqual(len(errors), 1)
        self.assertIn("gamenode-plugin-epic.exe", errors[0])

    def test_existing_license_and_pc_ports_resources_are_required(self):
        self._windows()
        (self.root / "licenses/NOTICE.txt").unlink()
        (self.root / "backup-assets/build-inputs.json").unlink()
        errors = package_check.verify(self.root, "windows")
        self.assertTrue(any("license notices" in error for error in errors))
        self.assertTrue(any("backup-assets/build-inputs.json" in error for error in errors))

    def test_obsolete_prototypes_are_not_distribution_inputs(self):
        self._windows()
        (self.root / "gamenode-daemon.exe").write_bytes(b"old prototype")
        self.assertIn("obsolete prototype", package_check.verify(self.root, "windows")[0])


if __name__ == "__main__":
    unittest.main()
