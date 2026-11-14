import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "scripts" / "run-desktop.sh"
# Every script that builds, packages or verifies bundled executables. The helper is exempt
# because it is the one place that reads executable names from the trusted descriptors.
SCRIPT_NAMES = (
    "run-desktop.sh",
    "run-desktop.ps1",
    "build-windows-wsl.sh",
    "dev-windows-wsl.sh",
    "dev-windows-wsl.ps1",
    "package-linux.sh",
    "package-windows-wsl.sh",
    "verify-plugin-package.py",
)


class DesktopLauncherTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.fake_bin = self.root / "bin"
        self.fake_bin.mkdir()
        self.target = self.root / "cargo-target"
        self.command_log = self.root / "cargo.log"
        self.app_log = self.root / "app-args.json"
        cargo = self.fake_bin / "cargo"
        cargo.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, pathlib, sys\n"
            "args = sys.argv[1:]\n"
            "with open(os.environ['CARGO_LOG'], 'a') as f: f.write(json.dumps(args) + '\\n')\n"
            "target = pathlib.Path(os.environ['FAKE_TARGET_DIR'])\n"
            "platform = os.environ.get('FAKE_PLATFORM', 'linux')\n"
            "names = ('gamenode', 'gamenode-plugin-steam', 'gamenode-plugin-epic', 'gamenode-plugin-packagekit')\n"
            "if args[0] == 'metadata':\n"
            "  print(json.dumps({'target_directory': str(target), 'packages': [{'name': name, 'targets': [{'name': name, 'kind': ['bin']}]} for name in names]}))\n"
            "elif args[0] == 'build':\n"
            "  profile = 'release' if '--release' in args else 'debug'\n"
            "  out = target / profile; out.mkdir(parents=True, exist_ok=True)\n"
            "  desktop = out / ('gamenode.exe' if platform == 'windows' else 'gamenode')\n"
            "  desktop.write_text('#!/usr/bin/env python3\\nimport json,os,pathlib,sys; pathlib.Path(os.environ[\"APP_ARG_LOG\"]).write_text(json.dumps(sys.argv[1:]))\\n'); desktop.chmod(0o755)\n"
            "  plugins = ('gamenode-plugin-steam.exe', 'gamenode-plugin-epic.exe') if platform == 'windows' else ('gamenode-plugin-steam', 'gamenode-plugin-packagekit')\n"
            "  if os.environ.get('OMIT_PLUGIN') == '1': plugins = ()\n"
            "  for name in plugins:\n"
            "    if name == 'gamenode-plugin-epic.exe' and os.environ.get('OMIT_EPIC') == '1': continue\n"
            "    (out / name).write_text('plugin')\n"
            "else: sys.exit(2)\n",
            encoding="utf-8",
        )
        cargo.chmod(0o755)
        self.env = os.environ.copy()
        self.env.update(
            {
                "PATH": f"{self.fake_bin}:{self.env['PATH']}",
                "FAKE_TARGET_DIR": str(self.target),
                "CARGO_LOG": str(self.command_log),
                "APP_ARG_LOG": str(self.app_log),
            }
        )

    def tearDown(self):
        self.temp.cleanup()

    def _cargo_calls(self):
        return [json.loads(line) for line in self.command_log.read_text().splitlines()]

    def _builds(self):
        return [call for call in self._cargo_calls() if call and call[0] == "build"]

    def test_linux_builds_host_and_descriptor_plugins_then_passes_app_args(self):
        result = subprocess.run(
            [str(LAUNCHER), "--release", "--no-default-features", "--", "--background"],
            cwd=ROOT,
            env=self.env,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self._builds(),
            [
                ["build", "--locked", "--bins", "-p", "gamenode-app",
                 "--release", "--no-default-features"],
                ["build", "--locked", "--bin", "gamenode-plugin-packagekit",
                 "--bin", "gamenode-plugin-steam", "--release", "--no-default-features"],
            ],
        )
        self.assertEqual(json.loads(self.app_log.read_text()), ["--background"])
        self.assertTrue((self.target / "release" / "gamenode-plugin-steam").is_file())
        self.assertTrue((self.target / "release" / "gamenode-plugin-packagekit").is_file())
        self.assertFalse((self.target / "release" / "gamenode-plugin-epic").exists())

    def _use_windows_host(self):
        uname = self.fake_bin / "uname"
        uname.write_text("#!/usr/bin/env bash\necho MINGW64_NT-10.0\n", encoding="utf-8")
        uname.chmod(0o755)
        self.env["FAKE_PLATFORM"] = "windows"

    def test_windows_builds_the_descriptor_plugins_beside_the_host(self):
        self._use_windows_host()
        result = subprocess.run(
            [str(LAUNCHER)], cwd=ROOT, env=self.env, capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            self._builds(),
            [
                ["build", "--locked", "--bins", "-p", "gamenode-app"],
                ["build", "--locked", "--bin", "gamenode-plugin-epic",
                 "--bin", "gamenode-plugin-steam"],
            ],
        )
        self.assertTrue((self.target / "debug" / "gamenode-plugin-epic.exe").is_file())
        self.assertEqual(json.loads(self.app_log.read_text()), [])

    def test_windows_requires_every_descriptor_plugin_before_launching(self):
        self._use_windows_host()
        self.env["OMIT_EPIC"] = "1"
        result = subprocess.run(
            [str(LAUNCHER)], cwd=ROOT, env=self.env, capture_output=True, text=True
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Required provider plugin was not built beside the desktop", result.stderr)
        self.assertIn("gamenode-plugin-epic.exe", result.stderr)
        self.assertFalse(self.app_log.exists())

    def test_default_launch_handles_empty_cargo_and_app_arguments(self):
        result = subprocess.run(
            [str(LAUNCHER)], cwd=ROOT, env=self.env, capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(self.app_log.read_text()), [])
        for build in self._builds():
            self.assertNotIn("", build)

    def test_missing_adjacent_plugin_fails_without_launching_desktop(self):
        self.env["OMIT_PLUGIN"] = "1"
        result = subprocess.run(
            [str(LAUNCHER)], cwd=ROOT, env=self.env, capture_output=True, text=True
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Required provider plugin was not built beside the desktop", result.stderr)
        self.assertIn("gamenode-plugin-packagekit", result.stderr)
        self.assertFalse(self.app_log.exists())

    def test_no_executable_names_are_hardcoded_outside_the_helper(self):
        for name in SCRIPT_NAMES:
            with self.subTest(script=name):
                text = (ROOT / "scripts" / name).read_text(encoding="utf-8")
                self.assertNotIn("gamenode-plugin-", text)

    def test_powershell_launcher_requires_python_311_and_uses_the_helper(self):
        text = (ROOT / "scripts" / "run-desktop.ps1").read_text(encoding="utf-8")
        self.assertIn("'py', '-3'", text)
        self.assertIn("'python'", text)
        self.assertIn("Python 3.11+", text)
        self.assertIn("--format cargo-args", text)
        self.assertIn("--format files", text)
        self.assertIn("& cargo build", text)

    def test_dev_windows_launcher_reads_the_generated_index(self):
        text = (ROOT / "scripts" / "dev-windows-wsl.ps1").read_text(encoding="utf-8")
        self.assertIn("bundled-plugins.json", text)
        self.assertIn("ConvertFrom-Json", text)
        self.assertNotIn(".toml", text)

    def test_dev_windows_build_writes_the_generated_index(self):
        text = (ROOT / "scripts" / "dev-windows-wsl.sh").read_text(encoding="utf-8")
        self.assertIn("bundled_plugins.write_index", text)
        self.assertIn("--format cargo-args", text)

    def test_windows_build_script_derives_plugins_from_the_helper(self):
        text = (ROOT / "scripts" / "build-windows-wsl.sh").read_text(encoding="utf-8")
        self.assertIn("--platform windows --format cargo-args", text)
        self.assertIn("--platform windows --format files", text)
        self.assertIn("bundled_plugins.write_index", text)

    def test_package_scripts_write_the_generated_index(self):
        for name in ("package-linux.sh", "package-windows-wsl.sh"):
            with self.subTest(script=name):
                text = (ROOT / "scripts" / name).read_text(encoding="utf-8")
                self.assertIn("bundled_plugins.write_index", text)
                self.assertNotIn("steamPluginSha256", text)
                self.assertNotIn("epicPluginSha256", text)


if __name__ == "__main__":
    unittest.main()
