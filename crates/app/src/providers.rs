//! The provider manifests embedded at build time from the repository's `providers/` directory.

use gamenode_core::provider::{self, LoadedProviders, Platform, registry::PluginBackend};

include!(concat!(env!("OUT_DIR"), "/providers.rs"));

/// Loads the bundled manifests that apply to `platform` and logs one warning per invalid one.
pub fn load_bundled(platform: Platform) -> LoadedProviders {
    provider::load(BUNDLED_MANIFESTS, platform)
}

/// Loads bundled manifests with the plugins approved by the application host.
pub fn load_bundled_with_plugins(platform: Platform, plugins: &[PluginBackend]) -> LoadedProviders {
    provider::load_with_plugins(BUNDLED_MANIFESTS, platform, plugins)
}

#[cfg(test)]
pub(crate) fn load_bundled_for_tests(platform: Platform) -> LoadedProviders {
    load_bundled_with_plugins(platform, &crate::plugin_host::backends(platform))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_file_in_providers_is_embedded() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../providers");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".toml"))
            .collect();
        on_disk.sort();
        let embedded: Vec<&str> = BUNDLED_MANIFESTS.iter().map(|(name, _)| *name).collect();
        assert_eq!(embedded, on_disk);
        assert!(embedded.contains(&"steam.toml"));
        assert!(embedded.contains(&"packagekit.toml"));
    }

    #[test]
    fn the_manual_provider_is_bundled_on_windows_and_linux() {
        for platform in [Platform::Windows, Platform::Linux] {
            let loaded = load_bundled_for_tests(platform);
            let provider = loaded.get("manual").expect("the manual provider");
            assert!(
                provider.is_manual_executable(),
                "manual launches local registrations on {platform:?}"
            );
            assert!(provider.has_launch());
        }
    }

    #[test]
    fn bundled_plugin_backends_come_from_descriptors() {
        for platform in [Platform::Windows, Platform::Linux] {
            let loaded = load_bundled_for_tests(platform);
            assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
            for descriptor in crate::plugin_host::descriptors() {
                assert_eq!(
                    loaded.get(&descriptor.provider_id).is_some(),
                    descriptor.platforms.contains(&platform),
                    "{} on {platform:?}",
                    descriptor.provider_id
                );
            }
        }
    }

    #[test]
    fn third_party_registry_states_do_not_change_official_linux_providers() {
        let platform = Platform::Linux;
        let official = crate::plugin_host::backends(platform);
        let expected = load_bundled_with_plugins(platform, &official);
        assert!(
            expected.diagnostics.is_empty(),
            "{:?}",
            expected.diagnostics
        );
        let expected_steam = expected.get("steam").unwrap().manifest().clone();
        let expected_packagekit = expected.get("packagekit").unwrap().manifest().clone();

        // An empty or disabled third-party registry contributes no backend. Invalid/untrusted
        // entries must not be allowed to impersonate Steam, while an enabled, unrelated provider
        // may add only its own backend. The host also rejects duplicate provider registrations;
        // this matrix verifies the Core manifest loader preserves the official definitions for
        // every backend set that can reach it.
        let mut invalid = official.clone();
        invalid.push(PluginBackend {
            plugin_id: "org.untrusted.replacement".into(),
            provider_id: "steam".into(),
            discovery: true,
            catalog: true,
            counter: true,
            actions: true,
            achievements: true,
        });
        let mut enabled = official.clone();
        enabled.push(PluginBackend {
            plugin_id: "org.example.third-party".into(),
            provider_id: "example-store".into(),
            discovery: true,
            catalog: true,
            counter: true,
            actions: true,
            achievements: true,
        });

        for (state, backends) in [
            ("empty", official.clone()),
            ("invalid", invalid),
            ("enabled", enabled),
            ("disabled", official),
        ] {
            let loaded = load_bundled_with_plugins(platform, &backends);
            assert!(
                loaded.diagnostics.is_empty(),
                "{state}: {:?}",
                loaded.diagnostics
            );
            let steam = loaded.get("steam").expect("Steam remains registered");
            assert_eq!(
                steam.manifest(),
                &expected_steam,
                "Steam changed for {state}"
            );
            assert_eq!(
                steam.discovery_plugin(),
                Some("org.gamenode.plugin.steam-discovery"),
                "Steam discovery must remain plugin-backed for {state}"
            );
            assert_eq!(
                steam.catalog_backend(),
                Some("org.gamenode.plugin.steam-discovery"),
                "Steam catalog must remain plugin-backed for {state}"
            );
            assert_eq!(
                steam.counter_plugin(),
                Some("org.gamenode.plugin.steam-discovery"),
                "Steam counter must remain plugin-backed for {state}"
            );
            assert_eq!(
                steam.achievement_plugin(),
                Some("org.gamenode.plugin.steam-discovery"),
                "Steam achievements integration must remain unchanged for {state}"
            );

            let packagekit = loaded
                .get("packagekit")
                .expect("Linux package discovery remains registered");
            assert_eq!(
                packagekit.manifest(),
                &expected_packagekit,
                "PackageKit changed for {state}"
            );
            assert_eq!(
                packagekit.discovery_plugin(),
                Some("org.gamenode.plugin.packagekit-discovery"),
                "Linux package discovery must remain plugin-backed for {state}"
            );
        }
    }

    #[test]
    fn the_bundled_manifests_are_all_valid_on_every_platform() {
        for platform in [Platform::Windows, Platform::Linux] {
            let loaded = load_bundled_for_tests(platform);
            assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
            let steam = loaded.get("steam").expect("steam loads");
            assert!(steam.has_catalog());
            assert_eq!(
                steam.achievement_plugin(),
                Some("org.gamenode.plugin.steam-discovery")
            );
            assert_eq!(
                steam.catalog_backend(),
                Some("org.gamenode.plugin.steam-discovery")
            );
            if platform == Platform::Linux {
                let packagekit = loaded.get("packagekit").unwrap();
                assert_eq!(packagekit.name(), "Gerenciador de pacotes Linux");
                assert_eq!(
                    packagekit.manifest().scan.plugin.as_deref(),
                    Some("org.gamenode.plugin.packagekit-discovery")
                );
                assert!(packagekit.manifest().catalog.is_none());
            } else {
                assert!(loaded.get("packagekit").is_none());
            }
        }
    }
}
