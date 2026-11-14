import re
from pathlib import Path
import unittest


WINDOWS_PACKAGING = Path(__file__).resolve().parents[2] / "packaging" / "windows"
INSTALLER = WINDOWS_PACKAGING / "install.ps1"
UNINSTALLER = WINDOWS_PACKAGING / "uninstall.ps1"
COPY_LIST = re.compile(r"foreach \(\$name in @\((?P<items>.*?)\)\)", re.DOTALL)
LIST_ENTRY = re.compile(r"'([^']+)'")
PLUGIN_NAME = re.compile(r"gamenode-plugin-[a-z0-9-]+")


def script_text(script):
    return script.read_text(encoding="utf-8")


def listed_names(script):
    text = script_text(script)
    matches = COPY_LIST.findall(text)
    if not matches:
        raise AssertionError(f"{script.name} has no foreach copy or removal list")
    return {
        entry.group(1)
        for items in matches
        for entry in LIST_ENTRY.finditer(items)
    }


def position(text, fragment):
    index = text.find(fragment)
    if index < 0:
        raise AssertionError(f"expected {fragment!r} in the script")
    return index


class WindowsInstallerIndex(unittest.TestCase):
    def setUp(self):
        self.install = script_text(INSTALLER)
        self.uninstall = script_text(UNINSTALLER)

    def test_installer_reads_the_package_plugin_index(self):
        self.assertIn("bundled-plugins.json", self.install)
        self.assertIn(
            "Get-Content -LiteralPath $indexPath -Raw | ConvertFrom-Json",
            self.install,
        )
        self.assertIn("$pluginFiles = @($index.plugins", self.install)
        self.assertIn("foreach ($file in $pluginFiles)", self.install)
        self.assertIn(
            "Copy-Item -LiteralPath (Join-Path $SourceDirectory $file) "
            "-Destination $installDir -Force",
            self.install,
        )

    def test_no_windows_workflow_keeps_its_own_plugin_names(self):
        self.assertEqual(PLUGIN_NAME.findall(self.install), [])
        self.assertEqual(PLUGIN_NAME.findall(self.uninstall), [])

    def test_installer_rejects_a_missing_indexed_file_before_install_changes(self):
        missing_file = position(
            self.install, 'throw "Package is missing indexed plugin: $file"'
        )
        file_check = position(
            self.install, "Test-Path -LiteralPath $sourcePlugin -PathType Leaf"
        )
        create_install_dir = position(
            self.install, "New-Item -ItemType Directory -Path $installDir"
        )
        uri_backup = position(self.install, "& reg.exe export")
        plugin_copy = position(
            self.install,
            "Copy-Item -LiteralPath (Join-Path $SourceDirectory $file) -Destination $installDir",
        )

        self.assertLess(file_check, missing_file)
        self.assertLess(missing_file, create_install_dir)
        self.assertLess(missing_file, uri_backup)
        self.assertLess(missing_file, plugin_copy)

    def test_installer_records_installed_plugins_and_carries_previous_records(self):
        self.assertIn(
            "$installedPlugins = @($pluginFiles + $previousPlugins | Select-Object -Unique)",
            self.install,
        )
        self.assertIn("plugins = @($installedPlugins)", self.install)
        # A plugin dropped by the new package must stay in the record so uninstall removes it.
        self.assertIn("foreach ($file in @($owned.plugins))", self.install)
        self.assertIn("$previousPlugins += $file", self.install)

    def test_installer_copies_the_host_and_the_index(self):
        names = listed_names(INSTALLER)
        self.assertIn("gamenode.exe", names)
        self.assertIn("bundled-plugins.json", names)

    def test_uninstaller_removes_the_union_of_marker_and_index_plugins(self):
        self.assertIn("foreach ($file in @($owned.plugins))", self.uninstall)
        self.assertIn("$pluginFiles += $file", self.uninstall)
        self.assertIn(
            "$indexPath = Join-Path $installDir 'bundled-plugins.json'",
            self.uninstall,
        )
        self.assertIn("foreach ($plugin in @($index.plugins))", self.uninstall)
        self.assertIn("$pluginFiles = @($pluginFiles | Select-Object -Unique)", self.uninstall)
        self.assertIn("foreach ($file in $pluginFiles)", self.uninstall)
        self.assertIn("Remove-Item -LiteralPath $path -Force", self.uninstall)

        marker_loop = position(self.uninstall, "foreach ($file in @($owned.plugins))")
        index_read = position(
            self.uninstall, "$indexPath = Join-Path $installDir 'bundled-plugins.json'"
        )
        self.assertLess(marker_loop, index_read)

    def test_uninstaller_falls_back_to_the_index_for_an_old_marker(self):
        # `@($owned.plugins)` is empty for an old marker without the field, and the index is read
        # unconditionally, so an old installation still removes the indexed plugins.
        self.assertIn("foreach ($file in @($owned.plugins))", self.uninstall)
        self.assertNotIn("if ($owned.plugins)", self.uninstall)
        self.assertIn("if (Test-Path -LiteralPath $indexPath)", self.uninstall)

    def test_uninstaller_removes_the_host_buffer_and_installation_record(self):
        names = listed_names(UNINSTALLER)
        self.assertIn("gamenode.exe", names)
        self.assertIn("bundled-plugins.json", names)
        self.assertIn(".gamenode-desktop-install.json", names)


if __name__ == "__main__":
    unittest.main()
