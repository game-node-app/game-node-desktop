use super::*;
use crate::provider::memory::{FixedRoots, MemoryFiles};

pub(crate) const STEAM_MANIFEST: &str = include_str!("../../../../../providers/steam.toml");
pub(crate) const EPIC_MANIFEST: &str = include_str!("../../../../../providers/epic.toml");

// The application supplies this registry from trusted distribution metadata.
pub(crate) fn load(sources: &[(&str, &str)], platform: Platform) -> LoadedProviders {
    super::load_with_plugins(
        sources,
        platform,
        &[PluginBackend {
            plugin_id: "org.gamenode.plugin.steam-discovery".into(),
            provider_id: "steam".into(),
            discovery: true,
            catalog: true,
            counter: true,
            actions: true,
            achievements: true,
        }],
    )
}

const MINIMAL: &str = r#"
id = "{id}"
name = "Test"
platforms = ["windows", "linux"]
source = "test"
scan = { native = "manual" }
"#;

fn minimal(id: &str) -> String {
    MINIMAL.replace("{id}", id)
}

#[test]
fn plugin_backends_are_provider_scoped_and_do_not_run_native_fallbacks() {
    let text = r#"
id = "example"
name = "Example store"
platforms = ["windows", "linux"]
source = "example"
scan = { plugin = "org.example.provider" }
catalog = { plugin = "org.example.provider" }
playtime = { plugin = "org.example.provider" }
launch = { uri = "example://run/{id}" }
"#;
    let approved = PluginBackend {
        plugin_id: "org.example.provider".into(),
        provider_id: "example".into(),
        discovery: true,
        catalog: true,
        counter: true,
        actions: true,
        achievements: true,
    };
    let loaded = load_with_plugins(
        &[("example.toml", text)],
        Platform::Windows,
        std::slice::from_ref(&approved),
    );
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let provider = loaded.get("example").unwrap();
    assert_eq!(provider.discovery_plugin(), Some("org.example.provider"));
    assert_eq!(provider.counter_plugin(), Some("org.example.provider"));
    assert_eq!(provider.catalog_backend(), Some("org.example.provider"));
    assert!(
        provider
            .scan(
                Platform::Windows,
                &MemoryFiles::new(),
                &FixedRoots::with_home("C:\\Users\\test")
            )
            .is_err()
    );
    for denied in [
        PluginBackend {
            provider_id: "other".into(),
            ..approved.clone()
        },
        PluginBackend {
            catalog: false,
            ..approved.clone()
        },
        PluginBackend {
            counter: false,
            ..approved.clone()
        },
        PluginBackend {
            actions: false,
            ..approved.clone()
        },
    ] {
        let loaded = load_with_plugins(&[("example.toml", text)], Platform::Windows, &[denied]);
        assert!(loaded.providers.is_empty());
        assert_eq!(loaded.diagnostics.len(), 1);
    }
    assert!(
        load(&[("example.toml", text)], Platform::Windows)
            .providers
            .is_empty()
    );
    let mixed = text.replace("scan = { plugin", "scan = { native = \"manual\", plugin");
    assert!(
        load_with_plugins(&[("mixed.toml", &mixed)], Platform::Windows, &[approved])
            .providers
            .is_empty()
    );
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
    assert!(steam.has_catalog());
    assert_eq!(
        steam.achievement_plugin(),
        Some("org.gamenode.plugin.steam-discovery")
    );
    assert_eq!(
        steam.catalog_backend(),
        Some("org.gamenode.plugin.steam-discovery")
    );
    assert!(steam.manifest().exclude_ids.contains(&"228980".to_owned()));
    assert_eq!(steam.manifest().roots.windows.len(), 1);
    assert_eq!(steam.manifest().roots.linux.len(), 3);
}

#[test]
fn steam_roots_resolve_per_platform() {
    let loaded = load(&[("steam.toml", STEAM_MANIFEST)], Platform::Windows);
    let steam = loaded.get("steam").unwrap();
    let windows = FixedRoots::with_home("C:\\Users\\me").with_registry(
        "HKCU\\Software\\Valve\\Steam",
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
fn platform_filtered_plugin_manifest_does_not_require_another_platform_registry_entry() {
    let packagekit = r#"
id = "packagekit"
name = "Linux package manager"
platforms = ["linux"]
source = "packagekit"
scan = { plugin = "org.gamenode.plugin.packagekit-discovery" }
"#;
    let linux_registry = [PluginBackend {
        plugin_id: "org.gamenode.plugin.packagekit-discovery".into(),
        provider_id: "packagekit".into(),
        discovery: true,
        catalog: false,
        counter: false,
        actions: false,
        achievements: false,
    }];
    let windows = load_with_plugins(&[("packagekit.toml", packagekit)], Platform::Windows, &[]);
    assert!(windows.providers.is_empty());
    assert!(windows.diagnostics.is_empty());

    let linux = load_with_plugins(
        &[("packagekit.toml", packagekit)],
        Platform::Linux,
        &linux_registry,
    );
    assert!(linux.diagnostics.is_empty(), "{:?}", linux.diagnostics);
    assert!(linux.get("packagekit").is_some());
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
            "unknown counter",
            format!("{}\nplaytime = {{ counter = \"missing\" }}", minimal("bad")),
            "unknown playtime counter `missing`",
        ),
        (
            "unknown scanner",
            minimal("bad").replace("native = \"manual\"", "native = \"nope\""),
            "unknown native scanner `nope`",
        ),
        (
            "unknown catalog backend",
            format!("{}\ncatalog = {{ native = \"nope\" }}", minimal("bad")),
            "unknown catalog backend `nope`",
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
            "registry outside hkcu and hklm",
            format!(
                "{}\n[[roots.windows]]\nregistry = 'HKCR\\X'\nvalue = \"v\"",
                minimal("bad")
            ),
            "must start with `HKCU\\` or `HKLM\\`",
        ),
        (
            "registry without a subkey",
            format!(
                "{}\n[[roots.windows]]\nregistry = 'HKLM\\'\nvalue = \"v\"",
                minimal("bad")
            ),
            "must start with `HKCU\\` or `HKLM\\`",
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
    let text = minimal("shop").replace("native = \"manual\"", "native = \"nope\"");
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
        "steam://nav/games/details/620"
    );
    assert!(steam.can_launch(&record) && steam.can_install(&record));

    let without_actions = minimal("plain");
    let loaded = load(&[("plain.toml", without_actions.as_str())], Platform::Linux);
    let plain = loaded.get("plain").unwrap();
    assert_eq!(plain.launch_uri(&record), Err(ActionError::Unsupported));
    assert!(!plain.can_launch(&record) && !plain.can_install(&record));
    assert!(!plain.has_catalog());
    assert_eq!(plain.catalog_backend(), None);
}

#[test]
fn a_known_catalog_backend_is_accepted_and_exposed() {
    let loaded = load(&[("steam.toml", STEAM_MANIFEST)], Platform::Linux);
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    assert_eq!(
        loaded.get("steam").unwrap().catalog_backend(),
        Some("org.gamenode.plugin.steam-discovery")
    );
}

#[test]
fn a_missing_install_scans_to_nothing() {
    let manifest = format!("{}\nexclude_ids = [\"2\"]", minimal("shop"));
    let loaded = load(&[("shop.toml", manifest.as_str())], Platform::Linux);
    let shop = loaded.get("shop").unwrap();
    let files = MemoryFiles::new();
    let roots = FixedRoots::with_home("/home/me");
    // An empty native/manual scanner succeeds without fabricated observations.
    assert_eq!(shop.scan(Platform::Linux, &files, &roots), Ok(vec![]));
}

#[test]
fn the_example_in_the_providers_doc_loads() {
    const DOC: &str = include_str!("../../../../../docs/providers.md");
    let start = DOC.find("```toml\n").expect("a toml example") + "```toml\n".len();
    let example = &DOC[start..start + DOC[start..].find("```").expect("closing fence")];

    for platform in [Platform::Windows, Platform::Linux] {
        let loaded = tests::load(&[("steam.toml", example)], platform);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        assert!(loaded.get("steam").is_some());
    }
}

#[test]
fn manual_provider_is_windows_only_and_executable_actions_are_restricted() {
    let text = include_str!("../../../../../providers/manual.toml");
    let loaded = load(&[("manual.toml", text)], Platform::Windows);
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let provider = loaded.get("manual").unwrap();
    assert!(provider.is_manual_executable());
    assert!(provider.has_launch());
    assert!(!provider.has_install());
    assert!(provider.external_category().is_none());
    assert!(
        load(&[("manual.toml", text)], Platform::Linux)
            .providers
            .is_empty()
    );
    let invalid = text.replace("id = \"manual\"", "id = \"other\"");
    assert!(
        !load(&[("other.toml", &invalid)], Platform::Windows)
            .diagnostics
            .is_empty()
    );
}

#[test]
fn every_documented_provider_manifest_loads() {
    let doc = include_str!("../../../../../docs/providers.md");
    for block in doc.split("```toml\n").skip(1) {
        let example = block.split("```").next().unwrap();
        let loaded = tests::load(&[("example.toml", example)], Platform::Windows);
        assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
        assert_eq!(loaded.providers.len(), 1);
    }
}

#[test]
fn foreign_platform_manifests_validate_structure_without_requiring_local_plugins() {
    let manifest = include_str!("../../../../../providers/packagekit.toml");
    let valid = super::load_with_plugins(&[("packagekit.toml", manifest)], Platform::Windows, &[]);
    assert!(valid.providers.is_empty());
    assert!(valid.diagnostics.is_empty());
    let invalid = manifest.replace("name = \"Gerenciador de pacotes Linux\"", "name = \"\"");
    let invalid =
        super::load_with_plugins(&[("packagekit.toml", &invalid)], Platform::Windows, &[]);
    assert_eq!(invalid.diagnostics.len(), 1);
    assert!(invalid.diagnostics[0].problem.contains("name"));
}

#[test]
fn achievement_backend_requires_a_registered_granted_plugin() {
    let text = minimal("example") + "\nachievements = { plugin = \"org.example.provider\" }\n";
    let approved = PluginBackend {
        plugin_id: "org.example.provider".into(),
        provider_id: "example".into(),
        discovery: false,
        catalog: false,
        counter: false,
        actions: false,
        achievements: true,
    };
    let loaded = load_with_plugins(
        &[("example.toml", text.as_str())],
        Platform::Windows,
        std::slice::from_ref(&approved),
    );
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    assert_eq!(
        loaded.get("example").unwrap().achievement_plugin(),
        Some("org.example.provider")
    );

    let unregistered = PluginBackend {
        provider_id: "other".into(),
        ..approved.clone()
    };
    let loaded = load_with_plugins(
        &[("example.toml", text.as_str())],
        Platform::Windows,
        &[unregistered],
    );
    assert!(loaded.providers.is_empty());
    assert_eq!(loaded.diagnostics.len(), 1);

    let unauthorized = PluginBackend {
        achievements: false,
        ..approved
    };
    let loaded = load_with_plugins(
        &[("example.toml", text.as_str())],
        Platform::Windows,
        &[unauthorized],
    );
    assert!(loaded.providers.is_empty());

    let without = load(
        &[("example.toml", minimal("example").as_str())],
        Platform::Windows,
    );
    assert!(without.diagnostics.is_empty(), "{:?}", without.diagnostics);
    assert_eq!(without.get("example").unwrap().achievement_plugin(), None);
}

fn epic_backend() -> PluginBackend {
    PluginBackend {
        plugin_id: "org.gamenode.plugin.epic".into(),
        provider_id: "epic".into(),
        discovery: true,
        catalog: true,
        counter: true,
        actions: true,
        achievements: false,
    }
}

#[test]
fn loads_the_bundled_epic_manifest_with_its_namespace_match_and_hklm_root() {
    let loaded = load_with_plugins(
        &[("epic.toml", EPIC_MANIFEST)],
        Platform::Windows,
        &[epic_backend()],
    );
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    let epic = loaded.get("epic").expect("epic loads on Windows");
    assert_eq!(epic.name(), "Epic Games Store");
    assert_eq!(epic.source(), "epicgames");
    assert_eq!(epic.external_category(), Some(1001));
    assert_eq!(epic.uid_field(), Some("namespace"));
    assert_eq!(epic.discovery_plugin(), Some("org.gamenode.plugin.epic"));
    assert_eq!(epic.catalog_backend(), Some("org.gamenode.plugin.epic"));
    assert_eq!(epic.counter_plugin(), Some("org.gamenode.plugin.epic"));
    assert!(epic.has_launch() && epic.has_install() && epic.has_catalog());
    assert!(epic.manifest().exclude_ids.is_empty());

    let roots = &epic.manifest().roots.windows;
    assert_eq!(roots.len(), 2);
    assert_eq!(
        roots[0].registry.as_deref(),
        Some(r"HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher")
    );
    assert_eq!(roots[0].value.as_deref(), Some("AppDataPath"));
    assert_eq!(roots[0].path, None);
    assert_eq!(
        roots[1].path.as_deref(),
        Some(r"%ProgramData%\Epic\EpicGamesLauncher\Data")
    );
    assert_eq!(roots[1].registry, None);
    assert_eq!(roots[1].value, None);

    let record = Record {
        id: "Fortnite".to_owned(),
        name: Some("Fortnite".to_owned()),
        install_dir: r"C:\Games\Fortnite".to_owned(),
        fields: Default::default(),
    };
    assert_eq!(
        epic.launch_uri(&record).unwrap().as_str(),
        "com.epicgames.launcher://apps/Fortnite?action=launch&silent=true"
    );
    assert_eq!(
        epic.install_uri(&record).unwrap().as_str(),
        "com.epicgames.launcher://apps/Fortnite?action=install"
    );

    let linux = load_with_plugins(&[("epic.toml", EPIC_MANIFEST)], Platform::Linux, &[]);
    assert!(linux.providers.is_empty(), "Epic is Windows-only");
    assert!(linux.diagnostics.is_empty(), "{:?}", linux.diagnostics);
}

#[test]
fn epic_roots_resolve_a_machine_registry_value_then_the_program_data_fallback() {
    let loaded = load_with_plugins(
        &[("epic.toml", EPIC_MANIFEST)],
        Platform::Windows,
        &[epic_backend()],
    );
    let epic = loaded.get("epic").unwrap();
    let resolver = FixedRoots::default().with_registry(
        r"HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher",
        "AppDataPath",
        r"D:\Epic\Launcher\Data",
    );
    assert_eq!(
        epic.resolve_roots(Platform::Windows, &resolver),
        [
            r"D:\Epic\Launcher\Data".to_owned(),
            r"%ProgramData%\Epic\EpicGamesLauncher\Data".to_owned(),
        ]
    );
    assert!(epic.resolve_roots(Platform::Linux, &resolver).is_empty());
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EpicCredentialAlias {
    #[allow(dead_code)]
    keyring_account: String,
    #[allow(dead_code)]
    private_file: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EpicPluginDescriptor {
    provider_id: String,
    plugin_id: String,
    name: String,
    platforms: Vec<Platform>,
    capabilities: Vec<String>,
    executable: String,
    provider_manifest: String,
    #[serde(default)]
    login_policy: Option<String>,
    #[serde(default)]
    credential_alias: Option<EpicCredentialAlias>,
    native_access: Vec<String>,
    legacy_login_profiles: Vec<String>,
}

#[test]
fn the_epic_plugin_descriptor_is_windows_only_and_loads_its_manifest() {
    let descriptor: EpicPluginDescriptor =
        toml::from_str(include_str!("../../../../../plugins/epic.toml")).unwrap();
    assert_eq!(descriptor.provider_id, "epic");
    assert_eq!(descriptor.plugin_id, "org.gamenode.plugin.epic");
    assert_eq!(descriptor.name, "Epic Games Store");
    assert_eq!(descriptor.platforms, [Platform::Windows]);
    assert_eq!(descriptor.executable, "gamenode-plugin-epic");
    assert_eq!(descriptor.provider_manifest, "epic");
    assert_eq!(
        descriptor.login_policy.as_deref(),
        Some("crates/plugin-epic/assets/login.toml")
    );
    assert!(
        descriptor.credential_alias.is_none(),
        "Epic uses its own default provider slot"
    );
    assert_eq!(descriptor.native_access, ["files", "network", "registry"]);
    assert!(descriptor.legacy_login_profiles.is_empty());
    assert_eq!(
        descriptor.capabilities,
        [
            "provider.context.read",
            "library.discovery",
            "library.catalog",
            "account.state.read",
            "account.state.publish",
            "auth.webview",
            "auth.webview.page.read",
            "secrets.read",
            "secrets.write",
            "secrets.delete",
            "playtime.counter",
            "provider.actions.declare",
        ]
    );
    assert!(
        !descriptor
            .capabilities
            .iter()
            .any(|capability| capability.contains("cookies") || capability == "achievements.read"),
        "Epic has no cookie or achievement grant"
    );

    let backend = PluginBackend {
        plugin_id: descriptor.plugin_id.clone(),
        provider_id: descriptor.provider_id.clone(),
        discovery: true,
        catalog: true,
        counter: true,
        actions: true,
        achievements: false,
    };
    let windows = load_with_plugins(
        &[(descriptor.provider_manifest.as_str(), EPIC_MANIFEST)],
        Platform::Windows,
        &[backend],
    );
    assert!(windows.diagnostics.is_empty(), "{:?}", windows.diagnostics);
    let epic = windows
        .get("epic")
        .expect("the descriptor's manifest loads");
    assert!(
        descriptor
            .platforms
            .iter()
            .all(|platform| epic.manifest().platforms.contains(platform)),
        "the descriptor's platforms are a subset of the manifest's"
    );
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EpicLoginPolicy {
    start_url: String,
    #[serde(default)]
    cookie_urls: Vec<String>,
    probe_origin: String,
    title: String,
    allowed_hosts: Vec<String>,
    cookie_names: Vec<String>,
    probe_script: String,
    width: u32,
    height: u32,
    timeout_seconds: u64,
}

#[test]
fn the_epic_login_policy_is_cookie_free_and_reads_only_the_authorization_code() {
    let policy: EpicLoginPolicy = toml::from_str(include_str!(
        "../../../../../crates/plugin-epic/assets/login.toml"
    ))
    .unwrap();
    assert!(policy.cookie_urls.is_empty());
    assert!(policy.cookie_names.is_empty());
    assert_eq!(policy.probe_origin, "https://www.epicgames.com");
    assert_eq!(policy.title, "Epic Games");
    assert_eq!(
        policy.allowed_hosts,
        [
            "epicgames.com",
            "epicgames.dev",
            "unrealengine.com",
            "hcaptcha.com"
        ]
    );
    assert_eq!(policy.width, 600);
    assert_eq!(policy.height, 760);
    assert_eq!(policy.timeout_seconds, 600);
    assert_eq!(
        policy.start_url,
        "https://www.epicgames.com/id/login?redirectUrl=https%3A%2F%2Fwww.epicgames.com%2Fid%2Fapi%2Fredirect%3FclientId%3D34a02cf8f4414e29b15921876da36f9a%26responseType%3Dcode"
    );
    assert!(
        policy
            .probe_script
            .contains("window.location.pathname !== '/id/api/redirect'")
    );
    assert!(policy.probe_script.contains("authorizationCode"));
    assert!(policy.probe_script.contains("authorization_code"));
    assert!(!policy.probe_script.to_ascii_lowercase().contains("cookie"));
}
