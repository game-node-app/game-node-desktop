import importlib.util
import json
import os
from pathlib import Path
import struct
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

import bundled_plugins  # noqa: E402  (the scripts directory is not a package)

spec = importlib.util.spec_from_file_location(
    "package_check", SCRIPTS / "verify-plugin-package.py"
)
package_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_check)


class PackageLayout(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="gamenode package with spaces ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def _plugin_files(self, platform):
        return [
            plugin.file_name(platform) for plugin in bundled_plugins.load(platform)
        ]

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
        for name in ("gamenode.exe", *self._plugin_files("windows")):
            (self.root / name).write_bytes(header)
        self._resources(windows=True)
        bundled_plugins.write_index(self.root, "windows")

    def _linux(self):
        directory = self.root / "libexec"
        directory.mkdir()
        header = bytearray(64)
        header[:6] = b"\x7fELF\x02\x01"
        struct.pack_into("<H", header, 18, 62)
        for name in ("gamenode", *self._plugin_files("linux")):
            binary = directory / name
            binary.write_bytes(header)
            binary.chmod(0o755)
        self._resources(windows=False)
        bundled_plugins.write_index(directory, "linux")

    def _other_platform_plugin(self, platform):
        plugins = {}
        for plugin in (*bundled_plugins.load("linux"), *bundled_plugins.load("windows")):
            plugins[(plugin.provider_id, plugin.plugin_id)] = plugin
        return next(
            plugin for plugin in plugins.values() if platform not in plugin.platforms
        )

    def test_windows_requires_exactly_the_descriptor_plugins(self):
        self._windows()
        self.assertEqual(package_check.verify(self.root, "windows"), [])
        missing = self._plugin_files("windows")[1]
        (self.root / missing).unlink()
        errors = package_check.verify(self.root, "windows")
        self.assertIn("missing regular binary", errors[0])
        self.assertIn(missing, errors[0])

    def test_windows_rejects_wrong_target_and_other_platform_plugin(self):
        self._windows()
        corrupt = self.root / self._plugin_files("windows")[0]
        corrupt.write_bytes(b"\x7fELF")
        other = self._other_platform_plugin("windows")
        (self.root / other.executable).write_bytes(b"not supported")
        (self.root / (other.executable + ".exe")).write_bytes(b"not supported")
        errors = package_check.verify(self.root, "windows")
        self.assertTrue(any("PE binary" in error for error in errors))
        self.assertTrue(any("another platform" in error for error in errors))
        self.assertTrue(any(other.executable in error for error in errors))

    @unittest.skipUnless(os.name == "posix", "Linux execute bits require POSIX stat semantics")
    def test_linux_requires_adjacent_plugins_and_user_execute_permissions(self):
        self._linux()
        self.assertEqual(package_check.verify(self.root, "linux"), [])
        plugin = self.root / "libexec" / self._plugin_files("linux")[0]
        plugin.chmod(0o644)
        errors = package_check.verify(self.root, "linux")
        self.assertEqual(len(errors), 1)
        self.assertIn("not executable", errors[0])
        plugin.unlink()
        self.assertIn("missing regular binary", package_check.verify(self.root, "linux")[0])

    def test_linux_does_not_require_the_windows_only_plugin(self):
        self._linux()
        other = self._other_platform_plugin("linux")
        self.assertFalse((self.root / "libexec" / other.executable).exists())
        self.assertEqual(package_check.verify(self.root, "linux"), [])

    def test_linux_forbids_the_other_platform_plugin(self):
        self._linux()
        other = self._other_platform_plugin("linux")
        for name in (other.executable, other.executable + ".exe"):
            (self.root / "libexec" / name).write_bytes(b"\x7fELF")
        errors = package_check.verify(self.root, "linux")
        self.assertTrue(errors)
        self.assertTrue(all("another platform" in error for error in errors))

    def test_missing_index_fails_both_platforms(self):
        self._windows()
        (self.root / "bundled-plugins.json").unlink()
        errors = package_check.verify(self.root, "windows")
        self.assertIn("missing bundled plugin index", errors[0])

        self._linux()
        (self.root / "libexec/bundled-plugins.json").unlink()
        errors = package_check.verify(self.root, "linux")
        self.assertIn("missing bundled plugin index", errors[0])

    def test_index_must_match_the_descriptors(self):
        self._windows()
        index = self.root / "bundled-plugins.json"
        metadata = json.loads(index.read_text(encoding="utf-8"))
        metadata["plugins"] = metadata["plugins"][:-1]
        index.write_text(json.dumps(metadata), encoding="utf-8")
        errors = package_check.verify(self.root, "windows")
        self.assertTrue(any("does not match the descriptors" in error for error in errors))

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
