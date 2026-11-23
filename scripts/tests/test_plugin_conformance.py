import importlib.util
from pathlib import Path
import sys
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))

spec = importlib.util.spec_from_file_location(
    "plugin_conformance", SCRIPTS / "check-plugin-conformance.py"
)
conformance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(conformance)


class ProtocolSurface(unittest.TestCase):
    def test_services_and_methods_are_parsed(self):
        source = (
            "service PluginHost {\n"
            "  rpc DescribeApi (DescribeApiRequest) returns (DescribeApiReply);\n"
            "  rpc GetPermissions (PermissionsRequest) returns (PermissionsReply);\n"
            "}\n"
            "service ResourceHost {\n"
            "  rpc LibraryList (LibraryListRequest) returns (LibraryListReply);\n"
            "}\n"
        )
        self.assertEqual(
            conformance.parse_proto_services(source),
            {
                "PluginHost": ["DescribeApi", "GetPermissions"],
                "ResourceHost": ["LibraryList"],
            },
        )

    def test_served_tables_keep_their_order_and_duplicates(self):
        source = (
            "pub const SERVED: &[Served] = &[\n"
            '    Served::new("First", cap::A, true, "scope"),\n'
            '    Served::new(\n        "Second",\n        cap::B,\n        false,\n'
            '        "scope",\n    ),\n'
            '    Served::new("First", cap::A, true, "scope"),\n'
            "];\n"
        )
        self.assertEqual(
            conformance.parse_served(source, "SERVED"),
            ["First", "Second", "First"],
        )
        self.assertEqual(conformance.parse_served(source, "OTHER"), [])

    def test_plugin_host_routes_come_from_the_describe_table(self):
        source = (
            "methods: vec![\n"
            "    ApiMethod {\n"
            '        service: "PluginHost".into(),\n'
            '        method: "DescribeApi".into(),\n'
            "    },\n"
            "    ApiMethod {\n"
            '        service: "ResourceHost".into(),\n'
            '        method: "LibraryList".into(),\n'
            "    },\n"
            "]\n"
        )
        self.assertEqual(
            conformance.parse_plugin_host_routes(source), ["DescribeApi"]
        )

    def test_the_real_protocol_constants_are_consistent(self):
        constants = conformance.protocol_constants()
        self.assertLessEqual(constants["MIN_API_MINOR"], constants["API_MINOR"])
        self.assertLessEqual(constants["THIRD_PARTY_API_MINOR"], constants["API_MINOR"])
        self.assertEqual(constants["API_MAJOR"], 1)


if __name__ == "__main__":
    unittest.main()
