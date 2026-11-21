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


class GoDescriptors(unittest.TestCase):
    def test_escapes_are_decoded_the_way_protoc_gen_go_writes_them(self):
        self.assertEqual(
            conformance.decode_go_string(r"\n\f\x00\\"),
            b"\n\f\x00\\",
        )
        self.assertEqual(conformance.decode_go_string("plain"), b"plain")

    def test_concatenated_raw_descriptor_is_reassembled(self):
        source = (
            'const file_plugin_proto_rawDesc = "" +\n'
            '\t"\\n" +\n'
            '\t"\\fplugin.proto" +\n'
            '\t"\\x12\\x02v1"\n'
            "\n"
            "var file_plugin_proto_msgTypes = make([]protoimpl.MessageInfo, 1)\n"
        )
        self.assertEqual(
            conformance.go_raw_descriptor(source),
            b"\n\x0cplugin.proto\x12\x02v1",
        )

    def test_missing_descriptor_is_reported(self):
        with self.assertRaises(ValueError):
            conformance.go_raw_descriptor("package proto\n")

    def test_descriptor_set_is_split_on_length_delimited_fields(self):
        first = b"\x01\x02"
        second = b"\x03"
        data = (
            b"\x0a" + bytes([len(first)]) + first + b"\x0a" + bytes([len(second)]) + second
        )
        self.assertEqual(conformance.descriptor_set_files(data), [first, second])

    def test_truncated_descriptor_set_is_rejected(self):
        with self.assertRaises(ValueError):
            conformance.descriptor_set_files(b"\x0a\x05abc")

    def test_first_difference_names_the_offset(self):
        self.assertIn("byte 1", conformance.first_difference(b"ab", b"ac"))
        self.assertIn("length", conformance.first_difference(b"a", b"ab"))


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
