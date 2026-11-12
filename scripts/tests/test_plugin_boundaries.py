import importlib.util
from pathlib import Path
import tempfile
import unittest


CHECKER_PATH = Path(__file__).resolve().parents[1] / "check-plugin-boundaries.py"
spec = importlib.util.spec_from_file_location("plugin_boundaries", CHECKER_PATH)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


def package(name):
    return {"id": f"path+file:///fixture#{name}@0.1.0", "name": name}


def node(name, dependencies=()):
    package_id = f"path+file:///fixture#{name}@0.1.0"
    return {
        "id": package_id,
        "deps": [
            {
                "name": dependency,
                "pkg": f"path+file:///fixture#{dependency}@0.1.0",
                "dep_kinds": [{"kind": kind, "target": target}],
            }
            for dependency, kind, target in dependencies
        ],
    }


def fixture_metadata(extra_packages=(), extra_nodes=(), *, app_dependencies=()):
    names = [
        "gamenode-plugin-protocol",
        "gamenode-plugin-sdk",
        "gamenode-plugin-steam",
        "gamenode-plugin-packagekit",
        "gamenode-plugin-epic",
        "gamenode-app",
        *extra_packages,
    ]
    packages = [package(name) for name in names]
    nodes = [
        node("gamenode-plugin-protocol"),
        node("gamenode-plugin-sdk", [("gamenode-plugin-protocol", None, None)]),
        node("gamenode-plugin-steam", [("gamenode-plugin-sdk", None, None)]),
        node("gamenode-plugin-packagekit", [("gamenode-plugin-sdk", None, None)]),
        node("gamenode-plugin-epic", [("gamenode-plugin-sdk", None, None)]),
        node("gamenode-app", app_dependencies),
        *extra_nodes,
    ]
    return {"packages": packages, "resolve": {"nodes": nodes}}


class PluginBoundaryTests(unittest.TestCase):
    def test_public_crates_reject_transitive_internal_dependencies(self):
        for internal in sorted(checker.INTERNAL_APPLICATION_CRATES):
            with self.subTest(internal=internal):
                metadata = fixture_metadata(
                    ["plugin-helper", internal],
                    [
                        node("gamenode-plugin-protocol", [("plugin-helper", None, None)]),
                        node("plugin-helper", [(internal, None, None)]),
                    ],
                )
                errors = checker.check_metadata(metadata)
                self.assertTrue(any(internal in error for error in errors))

    def test_public_crates_reject_build_dependencies_on_app_internals(self):
        metadata = fixture_metadata(
            ["gamenode-app"],
            [node("gamenode-plugin-sdk", [("gamenode-app", "build", None)])],
        )
        errors = checker.check_metadata(metadata)
        self.assertTrue(any("gamenode-app" in error and "build" in error for error in errors))

    def test_public_crates_reject_windows_only_platform_internal_edges(self):
        metadata = fixture_metadata(
            ["gamenode-platform"],
            [node("gamenode-plugin-steam", [("gamenode-platform", None, "cfg(windows)")])],
        )
        errors = checker.check_metadata(metadata)
        self.assertTrue(any("cfg(windows)" in error for error in errors))

    def test_host_rejects_production_steam_but_allows_test_only_fixture(self):
        metadata = fixture_metadata(
            app_dependencies=[("gamenode-plugin-steam", "dev", None)]
        )
        self.assertEqual(checker.check_metadata(metadata), [])
        metadata["resolve"]["nodes"][-1]["deps"] = [
            {
                "name": "gamenode-plugin-steam",
                "pkg": "path+file:///fixture#gamenode-plugin-steam@0.1.0",
                "dep_kinds": [{"kind": None, "target": None}],
            }
        ]
        errors = checker.check_metadata(metadata)
        self.assertTrue(any("production graph" in error for error in errors))

    def test_host_rejects_transitive_production_steam_dependency(self):
        metadata = fixture_metadata(
            ["host-adapter"],
            [node("host-adapter", [("gamenode-plugin-steam", None, None)])],
            app_dependencies=[("host-adapter", None, None)],
        )
        errors = checker.check_metadata(metadata)
        self.assertTrue(any("production graph" in error for error in errors))

    def test_host_allows_package_manager_only_as_a_dev_dependency(self):
        metadata = fixture_metadata(
            app_dependencies=[("gamenode-plugin-packagekit", "dev", None)],
        )
        self.assertEqual(checker.check_metadata(metadata), [])

    def test_epic_is_a_public_and_forbidden_host_crate(self):
        self.assertIn("gamenode-plugin-epic", checker.PUBLIC_PLUGIN_CRATES)
        self.assertIn("gamenode-plugin-epic", checker.FORBIDDEN_HOST_PLUGIN_CRATES)

        public = fixture_metadata(
            ["gamenode-platform"],
            [node("gamenode-plugin-epic", [("gamenode-platform", None, None)])],
        )
        errors = checker.check_metadata(public)
        self.assertTrue(
            any("gamenode-plugin-epic" in error and "gamenode-platform" in error for error in errors)
        )

        host = fixture_metadata(
            app_dependencies=[("gamenode-plugin-epic", None, None)],
        )
        errors = checker.check_metadata(host)
        self.assertTrue(
            any("gamenode-plugin-epic" in error and "production graph" in error for error in errors)
        )


class StoreBoundarySourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="plugin store boundaries ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.allowlist = self.root / checker.ALLOWLIST_PATH
        self.allowlist.parent.mkdir(parents=True, exist_ok=True)
        self.allowlist.write_text("", encoding="utf-8")
        self.write_descriptor(
            "steam.toml",
            provider_id="steam",
            platforms=("linux", "windows"),
            login_policy="policies/steam.toml",
            extra='achievement_icon_hosts = ["cdn.akamai.steamstatic.com"]',
        )
        self.write_descriptor(
            "epic.toml",
            provider_id="epic",
            platforms=("windows",),
            login_policy="policies/epic.toml",
        )
        self.write_policy("steam.toml", ["steampowered.com", "steamcommunity.com"])
        self.write_policy("epic.toml", ["epicgames.com"])
        self.write_manifest(
            "steam.toml",
            launch="steam://rungameid/{id}",
            install="steam://nav/games/details/{id}",
        )
        self.write_manifest(
            "epic.toml",
            launch="com.epicgames.launcher://apps/{id}?action=launch",
            install="com.epicgames.launcher://apps/{id}?action=install",
        )

    def write_source(self, relative, content):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
        return path

    def write_descriptor(
        self,
        name,
        *,
        provider_id,
        platforms,
        login_policy=None,
        extra="",
    ):
        platform_list = ", ".join(f'"{platform}"' for platform in platforms)
        text = [
            f'provider_id = "{provider_id}"',
            f'plugin_id = "org.example.plugin.{provider_id}"',
            f'name = "Fixture {provider_id}"',
            f"platforms = [{platform_list}]",
            f'executable = "gamenode-plugin-{provider_id}"',
            'capabilities = ["provider.context.read", "library.discovery"]',
        ]
        if login_policy is not None:
            text.append(f'login_policy = "{login_policy}"')
        if extra:
            text.append(extra)
        path = self.root / "plugins" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(text) + "\n", encoding="utf-8")
        return path

    def write_policy(self, name, hosts):
        host_list = ", ".join(f'"{host}"' for host in hosts)
        path = self.root / "policies" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(f"allowed_hosts = [{host_list}]\n", encoding="utf-8")
        return path

    def write_manifest(self, name, *, launch=None, install=None):
        lines = [f'id = "{Path(name).stem}"', f'name = "Fixture {Path(name).stem}"']
        if launch is not None:
            lines.append(f'launch = {{ uri = "{launch}" }}')
        if install is not None:
            lines.append(f'install = {{ uri = "{install}" }}')
        path = self.root / "providers" / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        return path

    def scan(self):
        return checker.check_store_sources(self.root)

    def assert_violation(self, errors, path, rule):
        self.assertTrue(
            any(f"{path}:1" in error and rule in error for error in errors),
            errors,
        )

    def test_production_violations_fail_every_rule(self):
        cases = {
            "crates/api/src/client.rs": (
                'const BASE: &str = "https://store.steampowered.com/";\n',
                "store-host",
            ),
            "crates/core/src/provider.rs": (
                "use crate::steam::Client;\n",
                "store-module-path",
            ),
            "crates/core/src/launch.rs": (
                'let uri = "steam://rungameid/620";\n',
                "store-uri-scheme",
            ),
            "crates/app/src/commands.rs": (
                'if provider == "steam" { }\n',
                "provider-id-literal",
            ),
            "crates/platform/src/paths.rs": (
                "fn steam_credential_file() { }\n",
                "store-identifier",
            ),
        }
        for relative, (content, _) in cases.items():
            self.write_source(relative, content)

        errors = self.scan()
        for relative, (_, rule) in cases.items():
            with self.subTest(rule=rule):
                self.assert_violation(errors, relative, rule)

    def test_allowlist_excuses_an_exact_reviewed_line(self):
        self.write_source(
            "crates/core/src/drm_free/identity.rs",
            'steam: id("steam"),\n',
        )
        self.allowlist.write_text(
            "crates/core/src/drm_free/identity.rs:1 provider-id-literal -- "
            "Ludusavi manifest identity field, not a provider branch.\n",
            encoding="utf-8",
        )
        self.assertEqual(self.scan(), [])

    def test_test_only_code_is_ignored(self):
        self.write_source(
            "crates/core/src/lib.rs",
            """pub fn production() {}

#[cfg(test)]
mod tests {
    const HOST: &str = "https://store.steampowered.com/";
    const FIXTURE: &str = r#"
scan = { plugin = "fixture.plugin" }
"#;

    #[test]
    fn compares_provider() {
        assert_eq!("steam", "steam");
    }
}
""",
        )
        self.write_source(
            "crates/core/src/provider_tests.rs",
            'const PROVIDER: &str = "steam";\n',
        )
        self.write_source(
            "crates/core/src/tests/fixture.rs",
            'const PROVIDER: &str = "steam";\n',
        )
        self.write_source(
            "ui/src/features/settings/StoreCard.test.tsx",
            "const provider = 'steam';\n",
        )
        self.write_source("ui/src/mock/host.ts", "const provider = 'steam';\n")

        self.assertEqual(self.scan(), [])

    def test_cfg_any_test_is_not_treated_as_test_only(self):
        self.write_source(
            "crates/platform/src/host.rs",
            '#[cfg(any(windows, test))]\nconst PROVIDER: &str = "steam";\n',
        )
        errors = self.scan()
        self.assertTrue(any("provider-id-literal" in error for error in errors), errors)

    def test_stale_allowlist_entries_fail(self):
        self.allowlist.write_text(
            "crates/core/src/drm_free/identity.rs:1 provider-id-literal -- removed code.\n",
            encoding="utf-8",
        )
        errors = self.scan()
        self.assertTrue(any("stale store boundary allowlist entry" in error for error in errors))

    def test_malformed_allowlist_entries_fail(self):
        self.allowlist.write_text("not an allowlist entry\n", encoding="utf-8")
        errors = self.scan()
        self.assertTrue(
            any("malformed store boundary allowlist entry" in error for error in errors)
        )

    def test_new_store_id_literal_fails_without_checker_edits(self):
        self.write_descriptor(
            "newstore.toml", provider_id="newstore", platforms=("linux", "windows")
        )
        self.write_source(
            "crates/app/src/commands.rs",
            'if provider == "newstore" { }\n',
        )

        errors = self.scan()
        self.assert_violation(errors, "crates/app/src/commands.rs", "provider-id-literal")
        self.assertNotIn("newstore", CHECKER_PATH.read_text(encoding="utf-8"))

    def test_derived_hosts_and_uri_schemes_are_checked(self):
        self.write_descriptor(
            "newstore.toml",
            provider_id="newstore",
            platforms=("linux",),
            login_policy="policies/newstore.toml",
            extra='achievement_icon_hosts = ["cdn.newstore.example"]',
        )
        self.write_policy("newstore.toml", ["login.newstore.example"])
        self.write_manifest(
            "newstore.toml",
            launch="newstore-launcher://launch/{id}",
            install="newstore-install://install/{id}",
        )
        self.write_source(
            "crates/api/src/client.rs",
            'const ICON: &str = "https://cdn.newstore.example/icon.png";\n'
            'const LOGIN: &str = "https://login.newstore.example/";\n'
            'const LAUNCH: &str = "newstore-launcher://launch/1";\n'
            'const INSTALL: &str = "newstore-install://install/1";\n'
            'const OTHER: &str = "otherstore://launch/1";\n',
        )

        errors = self.scan()
        self.assertTrue(
            any("client.rs:1" in error and "store-host" in error for error in errors), errors
        )
        self.assertTrue(
            any("client.rs:2" in error and "store-host" in error for error in errors), errors
        )
        self.assertTrue(
            any("client.rs:3" in error and "store-uri-scheme" in error for error in errors), errors
        )
        self.assertTrue(
            any("client.rs:4" in error and "store-uri-scheme" in error for error in errors), errors
        )
        self.assertFalse(any("client.rs:5" in error for error in errors), errors)

    def test_vocabulary_does_not_fall_back_to_real_descriptors(self):
        # The real descriptors declare `packagekit`; the temporary set does not, so this literal
        # must not be reported. That proves the vocabulary comes from the scanned tree.
        self.write_source("crates/app/src/commands.rs", 'if provider == "packagekit" { }\n')
        self.assertEqual(self.scan(), [])


if __name__ == "__main__":
    unittest.main()
