import importlib.util
from pathlib import Path
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
        "gamenode-app",
        *extra_packages,
    ]
    packages = [package(name) for name in names]
    nodes = [
        node("gamenode-plugin-protocol"),
        node("gamenode-plugin-sdk", [("gamenode-plugin-protocol", None, None)]),
        node("gamenode-plugin-steam", [("gamenode-plugin-sdk", None, None)]),
        node("gamenode-plugin-packagekit", [("gamenode-plugin-sdk", None, None)]),
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


if __name__ == "__main__":
    unittest.main()
