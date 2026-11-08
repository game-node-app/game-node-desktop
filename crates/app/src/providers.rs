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
    let steam = PluginBackend {
        plugin_id: "org.gamenode.plugin.steam-discovery".into(),
        provider_id: "steam".into(),
        discovery: true,
        catalog: true,
        counter: true,
        actions: true,
    };
    load_bundled_with_plugins(platform, &[steam])
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
    }

    #[test]
    fn the_bundled_manifests_are_all_valid_on_every_platform() {
        for platform in [Platform::Windows, Platform::Linux] {
            let loaded = load_bundled_for_tests(platform);
            assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
            let steam = loaded.get("steam").expect("steam loads");
            assert!(steam.has_catalog());
            assert_eq!(
                steam.catalog_backend(),
                Some("org.gamenode.plugin.steam-discovery")
            );
        }
    }
}
