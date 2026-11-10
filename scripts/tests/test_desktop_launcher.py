import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
LAUNCHER = ROOT / "scripts" / "run-desktop.sh"


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
            "if args[0] == 'metadata': print(json.dumps({'target_directory': str(target)}))\n"
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

    def test_builds_locked_host_and_plugin_in_one_directory_then_passes_app_args(self):
        result = subprocess.run(
            [str(LAUNCHER), "--release", "--no-default-features", "--", "--background"],
            cwd=ROOT,
            env=self.env,
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        cargo_calls = [json.loads(line) for line in self.command_log.read_text().splitlines()]
        build = cargo_calls[0]
        self.assertEqual(build[0], "build")
        self.assertIn("--locked", build)
        self.assertIn("--bins", build)
        self.assertIn("gamenode-app", build)
        self.assertIn("gamenode-plugin-steam", build)
        self.assertIn("gamenode-plugin-packagekit", build)
        self.assertNotIn("gamenode-plugin-epic", build)
        self.assertEqual(json.loads(self.app_log.read_text()), ["--background"])
        self.assertTrue((self.target / "release" / "gamenode-plugin-steam").is_file())
        self.assertTrue((self.target / "release" / "gamenode-plugin-packagekit").is_file())
        self.assertFalse((self.target / "release" / "gamenode-plugin-epic").exists())

    def _use_windows_host(self):
        uname = self.fake_bin / "uname"
        uname.write_text("#!/usr/bin/env bash\necho MINGW64_NT-10.0\n", encoding="utf-8")
        uname.chmod(0o755)
        self.env["FAKE_PLATFORM"] = "windows"

    def test_windows_branch_builds_epic_plugin_beside_the_host(self):
        self._use_windows_host()
        result = subprocess.run(
            [str(LAUNCHER)], cwd=ROOT, env=self.env, capture_output=True, text=True
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        build = json.loads(self.command_log.read_text().splitlines()[0])
        self.assertIn("gamenode-plugin-steam", build)
        self.assertIn("gamenode-plugin-epic", build)
        self.assertNotIn("gamenode-plugin-packagekit", build)
        self.assertTrue((self.target / "debug" / "gamenode-plugin-epic.exe").is_file())
        self.assertEqual(json.loads(self.app_log.read_text()), [])

    def test_windows_branch_requires_epic_plugin_before_launching(self):
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
        build = json.loads(self.command_log.read_text().splitlines()[0])
        self.assertNotIn("", build)

    def test_missing_adjacent_plugin_fails_without_launching_desktop(self):
        self.env["OMIT_PLUGIN"] = "1"
        result = subprocess.run(
            [str(LAUNCHER)], cwd=ROOT, env=self.env, capture_output=True, text=True
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Required provider plugin was not built beside the desktop", result.stderr)
        self.assertFalse(self.app_log.exists())


if __name__ == "__main__":
    unittest.main()
