import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS))

import bundled_plugins  # noqa: E402  (the scripts directory is not a package)


BINARIES = {
    "gamenode",
    "gamenode-plugin-epic",
    "gamenode-plugin-packagekit",
    "gamenode-plugin-steam",
}


class DescriptorTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.descriptors = Path(self.temp.name)

    def _write(
        self,
        name,
        *,
        provider_id="example",
        plugin_id="org.gamenode.plugin.example",
        executable="gamenode-plugin-example",
        platforms=("linux",),
    ):
        platforms_toml = ", ".join(json.dumps(platform) for platform in platforms)
        path = self.descriptors / name
        path.write_text(
            f'provider_id = {json.dumps(provider_id)}\n'
            f'plugin_id = {json.dumps(plugin_id)}\n'
            f'executable = {json.dumps(executable)}\n'
            f"platforms = [{platforms_toml}]\n",
            encoding="utf-8",
        )
        return path

    def test_windows_yields_steam_and_epic(self):
        plugins = bundled_plugins.load("windows")
        self.assertEqual([plugin.provider_id for plugin in plugins], ["epic", "steam"])
        self.assertEqual(
            [plugin.file_name("windows") for plugin in plugins],
            ["gamenode-plugin-epic.exe", "gamenode-plugin-steam.exe"],
        )
        self.assertNotIn("packagekit", [plugin.provider_id for plugin in plugins])

    def test_linux_yields_steam_and_package_manager(self):
        plugins = bundled_plugins.load("linux")
        self.assertEqual(
            [plugin.provider_id for plugin in plugins], ["packagekit", "steam"]
        )
        self.assertEqual(
            [plugin.file_name("linux") for plugin in plugins],
            ["gamenode-plugin-packagekit", "gamenode-plugin-steam"],
        )
        self.assertNotIn("epic", [plugin.provider_id for plugin in plugins])

    def test_malformed_descriptor_fails_and_names_the_file(self):
        (self.descriptors / "broken.toml").write_text(
            'provider_id = "broken"\n', encoding="utf-8"
        )
        with self.assertRaises(bundled_plugins.DescriptorError) as caught:
            bundled_plugins.load("linux", self.descriptors, binary_names=BINARIES)
        self.assertIn("broken.toml", str(caught.exception))

    def test_path_like_executable_fails_and_names_the_file(self):
        self._write("path-like.toml", executable="../gamenode-plugin-example")
        with self.assertRaises(bundled_plugins.DescriptorError) as caught:
            bundled_plugins.load("linux", self.descriptors, binary_names=BINARIES)
        self.assertIn("path-like.toml", str(caught.exception))
        self.assertIn("../gamenode-plugin-example", str(caught.exception))

    def test_unknown_executable_must_be_a_cargo_binary_target(self):
        self._write("unknown-binary.toml", executable="gamenode-plugin-not-built")
        with self.assertRaises(bundled_plugins.DescriptorError) as caught:
            bundled_plugins.load(
                "linux", self.descriptors, binary_names={"gamenode-plugin-steam"}
            )
        self.assertIn("unknown-binary.toml", str(caught.exception))
        self.assertIn("not a binary target", str(caught.exception))

    def test_unknown_platform_fails_and_names_the_file(self):
        self._write("unknown-platform.toml", platforms=("macos",))
        with self.assertRaises(bundled_plugins.DescriptorError) as caught:
            bundled_plugins.load("linux", self.descriptors, binary_names=BINARIES)
        self.assertIn("unknown-platform.toml", str(caught.exception))
        self.assertIn("macos", str(caught.exception))

    def test_duplicate_provider_id_fails_and_names_both_descriptors(self):
        self._write("first.toml", executable="gamenode-plugin-example")
        self._write(
            "second.toml",
            provider_id="example",
            plugin_id="org.gamenode.plugin.second",
            executable="gamenode-plugin-example-two",
        )
        with self.assertRaises(bundled_plugins.DescriptorError) as caught:
            bundled_plugins.load(
                "linux",
                self.descriptors,
                binary_names={"gamenode-plugin-example", "gamenode-plugin-example-two"},
            )
        message = str(caught.exception)
        self.assertIn("second.toml", message)
        self.assertIn("duplicate provider id", message)

    def test_platform_filter_uses_the_declared_platforms(self):
        self._write("linux-only.toml", platforms=("linux",))
        self._write(
            "windows-only.toml",
            provider_id="windows-example",
            plugin_id="org.gamenode.plugin.windows-example",
            executable="gamenode-plugin-windows-example",
            platforms=("windows",),
        )
        binaries = {"gamenode-plugin-example", "gamenode-plugin-windows-example"}
        self.assertEqual(
            [plugin.provider_id for plugin in bundled_plugins.load(
                "linux", self.descriptors, binary_names=binaries
            )],
            ["example"],
        )
        self.assertEqual(
            [plugin.provider_id for plugin in bundled_plugins.load(
                "windows", self.descriptors, binary_names=binaries
            )],
            ["windows-example"],
        )


class CliTests(unittest.TestCase):
    def _run(self, *arguments):
        result = subprocess.run(
            [sys.executable, str(SCRIPTS / "bundled_plugins.py"), *arguments],
            cwd=ROOT,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_files_adds_exe_on_windows_only(self):
        self.assertEqual(
            self._run("--platform", "windows", "--format", "files").splitlines(),
            ["gamenode-plugin-epic.exe", "gamenode-plugin-steam.exe"],
        )
        self.assertEqual(
            self._run("--platform", "linux", "--format", "files").splitlines(),
            ["gamenode-plugin-packagekit", "gamenode-plugin-steam"],
        )

    def test_cargo_args_are_bin_pairs(self):
        self.assertEqual(
            self._run("--platform", "windows", "--format", "cargo-args").splitlines(),
            [
                "--bin",
                "gamenode-plugin-epic",
                "--bin",
                "gamenode-plugin-steam",
            ],
        )

    def test_json_describes_the_selected_platform(self):
        payload = json.loads(self._run("--platform", "linux", "--format", "json"))
        self.assertEqual(
            [entry["provider_id"] for entry in payload], ["packagekit", "steam"]
        )
        self.assertEqual(
            [entry["executable"] for entry in payload],
            ["gamenode-plugin-packagekit", "gamenode-plugin-steam"],
        )
        self.assertTrue(all(entry["platforms"] for entry in payload))

    def test_names_do_not_include_a_windows_suffix(self):
        self.assertEqual(
            self._run("--platform", "windows", "--format", "names").splitlines(),
            ["gamenode-plugin-epic", "gamenode-plugin-steam"],
        )

    def test_write_index_uses_the_schema_and_selected_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            destination = bundled_plugins.write_index(temporary, "windows")
            self.assertEqual(destination, Path(temporary) / "bundled-plugins.json")
            index = json.loads(destination.read_text(encoding="utf-8"))
        self.assertEqual(index["schema"], 1)
        self.assertEqual(
            index["plugins"],
            [
                {
                    "providerId": "epic",
                    "pluginId": "org.gamenode.plugin.epic",
                    "file": "gamenode-plugin-epic.exe",
                },
                {
                    "providerId": "steam",
                    "pluginId": "org.gamenode.plugin.steam-discovery",
                    "file": "gamenode-plugin-steam.exe",
                },
            ],
        )


if __name__ == "__main__":
    unittest.main()
