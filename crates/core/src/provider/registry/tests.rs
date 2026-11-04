use super::*;
use crate::provider::memory::{FixedRoots, MemoryFiles};

pub(crate) const STEAM_MANIFEST: &str = include_str!("../../../../../providers/steam.toml");

const MINIMAL: &str = r#"
id = "{id}"
name = "Test"
platforms = ["windows", "linux"]
source = "test"
scan = { native = "steam" }
"#;

fn minimal(id: &str) -> String {
    MINIMAL.replace("{id}", id)
}

#[test]
fn loads_the_bundled_steam_manifest() {
    let loaded = load(&[("steam.toml", STEAM_MANIFEST)], Platform::Windows);
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let steam = loaded.get("steam").expect("steam loads");
    assert_eq!(steam.name(), "Steam");
    assert_eq!(steam.source(), "steam");
    assert_eq!(steam.external_category(), Some(1));
    assert!(steam.has_launch() && steam.has_install());
    assert!(steam.manifest().exclude_ids.contains(&"228980".to_owned()));
    assert_eq!(steam.manifest().roots.windows.len(), 1);
    assert_eq!(steam.manifest().roots.linux.len(), 3);
}

#[test]
fn steam_roots_resolve_per_platform() {
    let loaded = load(&[("steam.toml", STEAM_MANIFEST)], Platform::Windows);
    let steam = loaded.get("steam").unwrap();
    let windows = FixedRoots::with_home("C:\\Users\\me").with_registry(
        "Software\\Valve\\Steam",
        "SteamPath",
        "c:/program files (x86)/steam",
    );
    assert_eq!(
        steam.resolve_roots(Platform::Windows, &windows),
        ["c:/program files (x86)/steam"]
    );
    let linux = FixedRoots::with_home("/home/me");
    assert_eq!(
        steam.resolve_roots(Platform::Linux, &linux),
        [
            "/home/me/.steam/steam",
            "/home/me/.local/share/Steam",
            "/home/me/.var/app/com.valvesoftware.Steam/.local/share/Steam"
        ]
    );
}

#[test]
fn skips_manifests_for_other_platforms() {
    let linux_only = minimal("shop").replace(r#"["windows", "linux"]"#, r#"["linux"]"#);
    let sources = [
        ("shop.toml", linux_only.as_str()),
        ("steam.toml", STEAM_MANIFEST),
    ];

    let windows = load(&sources, Platform::Windows);
    assert!(windows.get("shop").is_none(), "not loaded");
    assert!(windows.get("steam").is_some());
    assert!(windows.diagnostics.is_empty(), "not reported either");

    let linux = load(&sources, Platform::Linux);
    assert!(linux.get("shop").is_some());
}

#[test]
fn invalid_manifests_are_reported_one_each_and_the_rest_still_load() {
    let cases: Vec<(&str, String, &str)> = vec![
        (
            "unknown field",
            format!("{}\nextra = 1", minimal("bad")),
            "unknown field `extra`",
        ),
        (
            "missing field",
            minimal("bad").replace("source = \"test\"\n", ""),
            "missing field `source`",
        ),
        (
            "unknown scanner",
            minimal("bad").replace("native = \"steam\"", "native = \"nope\""),
            "unknown native scanner `nope`",
        ),
        (
            "malformed placeholder",
            format!(
                "{}\nlaunch = {{ uri = \"steam://run/{{Id}}\" }}",
                minimal("bad")
            ),
            "malformed placeholder",
        ),
        (
            "not a uri",
            format!("{}\nlaunch = {{ uri = \"run/{{id}}\" }}", minimal("bad")),
            "not a URI",
        ),
        (
            "placeholder in scheme",
            format!("{}\ninstall = {{ uri = \"{{id}}://x\" }}", minimal("bad")),
            "placeholder appears in the scheme",
        ),
        ("bad id", minimal("Bad Id"), "`id` must be"),
        (
            "unknown platform",
            minimal("bad").replace("\"linux\"]", "\"amiga\"]"),
            "unknown variant `amiga`",
        ),
        ("not toml", "this is = = not toml".to_owned(), "key"),
        (
            "registry on linux",
            format!(
                "{}\n[[roots.linux]]\nregistry = 'HKCU\\X'\nvalue = \"v\"",
                minimal("bad")
            ),
            "only valid under `roots.windows`",
        ),
        (
            "registry outside hkcu",
            format!(
                "{}\n[[roots.windows]]\nregistry = 'HKLM\\X'\nvalue = \"v\"",
                minimal("bad")
            ),
            "must start with `HKCU\\`",
        ),
        (
            "empty root",
            format!("{}\n[[roots.linux]]", minimal("bad")),
            "needs either `path`",
        ),
        (
            "empty platforms",
            minimal("bad").replace("[\"windows\", \"linux\"]", "[]"),
            "`platforms` is empty",
        ),
    ];

    for (what, text, expected) in cases {
        let loaded = load(
            &[
                ("a_bad.toml", text.as_str()),
                ("steam.toml", STEAM_MANIFEST),
            ],
            Platform::Linux,
        );
        assert_eq!(
            loaded.diagnostics.len(),
            1,
            "{what}: {:?}",
            loaded.diagnostics
        );
        let diagnostic = &loaded.diagnostics[0];
        assert_eq!(diagnostic.file, "a_bad.toml", "{what}");
        assert!(
            diagnostic.problem.contains(expected),
            "{what}: {:?} should contain {expected:?}",
            diagnostic.problem
        );
        assert!(
            loaded.get("steam").is_some(),
            "{what}: valid manifest still loads"
        );
    }
}

#[test]
fn diagnostics_name_the_provider_when_it_can_be_read() {
    let text = minimal("shop").replace("native = \"steam\"", "native = \"nope\"");
    let loaded = load(&[("shop.toml", text.as_str())], Platform::Linux);
    assert_eq!(loaded.diagnostics[0].provider.as_deref(), Some("shop"));
    let shown = loaded.diagnostics[0].to_string();
    assert!(shown.contains("shop.toml") && shown.contains("`shop`") && shown.contains("nope"));
}

#[test]
fn a_duplicate_provider_id_is_rejected_and_the_first_file_wins() {
    let first = minimal("dup");
    let second = minimal("dup").replace("name = \"Test\"", "name = \"Second\"");
    // Listed out of order on purpose: loading sorts by file name.
    let loaded = load(
        &[("b.toml", second.as_str()), ("a.toml", first.as_str())],
        Platform::Linux,
    );
    assert_eq!(loaded.providers.len(), 1);
    assert_eq!(loaded.providers[0].name(), "Test");
    assert_eq!(loaded.diagnostics.len(), 1);
    assert_eq!(loaded.diagnostics[0].file, "b.toml");
    assert!(
        loaded.diagnostics[0]
            .problem
            .contains("duplicate provider id")
    );
}

#[test]
fn actions_use_the_manifest_templates() {
    let loaded = load(&[("steam.toml", STEAM_MANIFEST)], Platform::Linux);
    let steam = loaded.get("steam").unwrap();
    let record = Record {
        id: "620".to_owned(),
        name: Some("Portal 2".to_owned()),
        install_dir: "/g/Portal 2".to_owned(),
        fields: Default::default(),
    };
    assert_eq!(
        steam.launch_uri(&record).unwrap().as_str(),
        "steam://rungameid/620"
    );
    assert_eq!(
        steam.install_uri(&record).unwrap().as_str(),
        "steam://install/620"
    );
    assert!(steam.can_launch(&record) && steam.can_install(&record));

    let without_actions = minimal("plain");
    let loaded = load(&[("plain.toml", without_actions.as_str())], Platform::Linux);
    let plain = loaded.get("plain").unwrap();
    assert_eq!(plain.launch_uri(&record), Err(ActionError::Unsupported));
    assert!(!plain.can_launch(&record) && !plain.can_install(&record));
}

#[test]
fn a_missing_install_scans_to_nothing() {
    let manifest = format!("{}\nexclude_ids = [\"2\"]", minimal("shop"));
    let loaded = load(&[("shop.toml", manifest.as_str())], Platform::Linux);
    let shop = loaded.get("shop").unwrap();
    let files = MemoryFiles::new();
    let roots = FixedRoots::with_home("/home/me");
    // No Steam installed: an empty scan, not an error.
    assert_eq!(shop.scan(Platform::Linux, &files, &roots), Ok(vec![]));
}

#[test]
fn the_example_in_the_providers_doc_loads() {
    const DOC: &str = include_str!("../../../../../docs/providers.md");
    let start = DOC.find("```toml\n").expect("a toml example") + "```toml\n".len();
    let example = &DOC[start..start + DOC[start..].find("```").expect("closing fence")];

    for platform in [Platform::Windows, Platform::Linux] {
        let loaded = load(&[("steam.toml", example)], platform);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        assert!(loaded.get("steam").is_some());
    }
}
