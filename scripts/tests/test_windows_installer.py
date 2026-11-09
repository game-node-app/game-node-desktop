import re
from pathlib import Path
import unittest


WINDOWS_PACKAGING = Path(__file__).resolve().parents[2] / "packaging" / "windows"
INSTALLER = WINDOWS_PACKAGING / "install.ps1"
UNINSTALLER = WINDOWS_PACKAGING / "uninstall.ps1"
REQUIRED_WINDOWS_BINARIES = ("gamenode.exe", "gamenode-plugin-steam.exe")
COPY_LIST = re.compile(r"foreach \(\$name in @\((?P<items>.*?)\)\)", re.DOTALL)
LIST_ENTRY = re.compile(r"'([^']+)'")


def listed_names(script):
    text = script.read_text(encoding="utf-8")
    match = COPY_LIST.search(text)
    if match is None:
        raise AssertionError(f"{script.name} has no foreach copy or removal list")
    return {entry.group(1) for entry in LIST_ENTRY.finditer(match.group("items"))}


class WindowsInstallerLists(unittest.TestCase):
    def test_installer_copies_required_windows_binaries(self):
        names = listed_names(INSTALLER)
        missing = [name for name in REQUIRED_WINDOWS_BINARIES if name not in names]
        self.assertEqual(
            missing, [], f"install.ps1 does not copy required binaries: {missing}"
        )

    def test_uninstaller_removes_required_windows_binaries(self):
        names = listed_names(UNINSTALLER)
        missing = [name for name in REQUIRED_WINDOWS_BINARIES if name not in names]
        self.assertEqual(
            missing, [], f"uninstall.ps1 does not remove required binaries: {missing}"
        )


if __name__ == "__main__":
    unittest.main()
