//! Composition of installed third-party provider packages into the desktop library.
//!
//! The official providers still come from the manifests embedded at build time. A signed package
//! accepted through the reviewed import flow has its own JSON manifest instead, so this host-side
//! step verifies the same registry, publisher pin and compatibility facts the runtime already
//! uses, then derives the minimal provider manifest the library scans:
//!
//! ```toml
//! id = "<provider_id>"
//! name = "<package name>"
//! platforms = ["<the running platform>"]
//! source = "community"
//!
//! [scan]
//! plugin = "<plugin_id>"
//! ```
//!
//! The official list, its order and its behavior are unchanged: third-party providers are
//! appended after it in plugin-id order, and an id that collides with an official provider is
//! refused. Everything that touches the registry, trust store or package files runs on blocking
//! workers.

use std::path::{Path, PathBuf};

use gamenode_core::provider::{self, Platform, PluginBackend, Provider};
use gamenode_platform::paths::DataPaths;
use gamenode_platform::plugin_environment::EmulationPolicy;
use gamenode_platform::plugin_registry::{InstalledPluginVersion, PluginPackageRegistry};
use gamenode_plugin_protocol::capability;

use super::managed_packages::ManagedPackageLifecycle;
use super::package::ValidatedManifest;
use super::profile::PluginStateLayout;
use super::publisher_trust::{KeyStatus, PublisherTrustStore};

/// The provider list the library starts (or continues) with, plus one diagnostic per package
/// that was deliberately skipped.
#[derive(Clone, Debug, Default)]
pub struct LibraryProviders {
    pub providers: Vec<Provider>,
    pub diagnostics: Vec<String>,
}

impl LibraryProviders {
    pub fn get(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|provider| provider.id() == id)
    }
}

/// One installed third-party provider that passed every host check.
struct ComposedProvider {
    plugin_id: String,
    provider_id: String,
    provider: Provider,
}

/// The providers the desktop library composes at startup: every official manifest that applies
/// to `platform`, followed by every active, trusted, publisher-pinned, compatible third-party
/// provider package in the registry.
pub async fn load_library_providers(platform: Platform, paths: &DataPaths) -> LibraryProviders {
    let mut loaded = crate::providers::load_bundled_with_plugins(
        platform,
        &crate::plugin_host::backends(platform),
    );
    let mut result = LibraryProviders {
        providers: std::mem::take(&mut loaded.providers),
        diagnostics: loaded.diagnostics.iter().map(ToString::to_string).collect(),
    };
    let official: std::collections::BTreeSet<String> = result
        .providers
        .iter()
        .map(|provider| provider.id().to_owned())
        .collect();

    let registry = paths.plugin_packages_dir();
    let trust_dir = PluginStateLayout::managed(paths).trust_dir();
    let composition = match open_registry(registry).await {
        Ok(registry) => compose(platform, registry, trust_dir, None).await,
        Err(problem) => Composition {
            diagnostics: vec![problem],
            ..Default::default()
        },
    };
    result.diagnostics.extend(composition.diagnostics);
    for entry in composition.eligible {
        if official.contains(&entry.provider_id) {
            result.diagnostics.push(format!(
                "third-party package `{}` was skipped: provider id `{}` is reserved by an official provider",
                entry.plugin_id, entry.provider_id
            ));
            continue;
        }
        if result.get(&entry.provider_id).is_some() {
            result.diagnostics.push(format!(
                "third-party package `{}` was skipped: provider id `{}` is already registered",
                entry.plugin_id, entry.provider_id
            ));
            continue;
        }
        result.providers.push(entry.provider);
    }
    for diagnostic in &result.diagnostics {
        tracing::warn!(%diagnostic, "library provider composition skipped a package");
    }
    result
}

/// Re-reads one installed package after an accepted import. Returns the provider to add to an
/// already running library, or `None` when the package is not a discovery provider.
pub async fn installed_library_provider(
    platform: Platform,
    paths: &DataPaths,
    plugin_id: &str,
) -> Result<Option<Provider>, String> {
    installed_provider_from_registry(
        platform,
        open_registry(paths.plugin_packages_dir()).await?,
        PluginStateLayout::managed(paths).trust_dir(),
        plugin_id,
    )
    .await
}

/// The same re-read for callers that already hold the import service's registry and trust paths.
pub(crate) async fn installed_provider_from_registry(
    platform: Platform,
    registry: PluginPackageRegistry,
    trust_dir: PathBuf,
    plugin_id: &str,
) -> Result<Option<Provider>, String> {
    let composition = compose(platform, registry, trust_dir, Some(plugin_id)).await;
    let mut providers = composition.eligible.into_iter();
    match (providers.next(), providers.next()) {
        (None, _) => Ok(None),
        (Some(entry), None) => Ok(Some(entry.provider)),
        (Some(_), Some(_)) => Err("the installed provider package is ambiguous".to_owned()),
    }
}

async fn open_registry(path: PathBuf) -> Result<PluginPackageRegistry, String> {
    tokio::task::spawn_blocking(move || PluginPackageRegistry::at(path))
        .await
        .map_err(|_| "plugin package registry worker failed".to_owned())?
        .map_err(|error| format!("plugin package registry is unavailable: {error}"))
}

#[derive(Default)]
struct Composition {
    eligible: Vec<ComposedProvider>,
    diagnostics: Vec<String>,
}

/// Runs the registry, trust and package checks for one provider or every active package.
async fn compose(
    platform: Platform,
    registry: PluginPackageRegistry,
    trust_dir: PathBuf,
    only: Option<&str>,
) -> Composition {
    let only = only.map(str::to_owned);
    let lookup = registry.clone();
    let installed = tokio::task::spawn_blocking(move || lookup.active_versions())
        .await
        .map_err(|_| "plugin package registry worker failed".to_owned())
        .and_then(|versions| {
            versions.map_err(|error| format!("plugin package registry is unavailable: {error}"))
        });
    let installed = match installed {
        Ok(installed) => installed,
        Err(error) => {
            return Composition {
                diagnostics: vec![error],
                ..Default::default()
            };
        }
    };
    let trust = tokio::task::spawn_blocking(move || PublisherTrustStore::open(trust_dir))
        .await
        .map_err(|_| "publisher trust worker failed".to_owned())
        .and_then(|trust| {
            trust.map_err(|error| format!("publisher trust store is unavailable: {error}"))
        });
    let trust = match trust {
        Ok(trust) => trust,
        Err(error) => {
            return Composition {
                diagnostics: vec![error],
                ..Default::default()
            };
        }
    };
    let packages = ManagedPackageLifecycle::new(registry);
    let mut composition = Composition::default();
    for installed in installed {
        if only
            .as_deref()
            .is_some_and(|plugin_id| plugin_id != installed.plugin_id)
        {
            continue;
        }
        let plugin_id = installed.plugin_id.clone();
        match compose_one(platform, installed, &trust, &packages).await {
            Ok(entry) => composition.eligible.push(entry),
            Err(problem) => {
                tracing::warn!(%problem, %plugin_id, "third-party provider package is not eligible for the library");
                composition
                    .diagnostics
                    .push(format!("package `{plugin_id}`: {problem}"));
            }
        }
    }
    composition
        .eligible
        .sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));
    composition
}

async fn compose_one(
    platform: Platform,
    installed: InstalledPluginVersion,
    trust: &PublisherTrustStore,
    packages: &ManagedPackageLifecycle,
) -> Result<ComposedProvider, String> {
    let manifest = read_manifest(&installed).await?;
    let package = manifest.manifest();
    if package.id != installed.plugin_id || package.version != installed.version {
        return Err(format!(
            "plugin package `{}` does not match its registry record",
            installed.plugin_id
        ));
    }
    if package.kind != super::package::PluginKind::Provider {
        return Err(format!(
            "plugin package `{}` is not a provider",
            installed.plugin_id
        ));
    }
    if !package
        .permissions
        .iter()
        .any(|permission| permission.id == capability::DISCOVERY)
    {
        return Err(format!(
            "plugin package `{}` does not declare `{}`",
            installed.plugin_id,
            capability::DISCOVERY
        ));
    }
    let provider_id = package
        .provider_id
        .clone()
        .ok_or_else(|| "provider package has no provider id".to_owned())?;
    let publisher_key = manifest.publisher_key();
    match trust
        .inspect(&installed.plugin_id, &publisher_key)
        .map_err(|error| format!("publisher trust status is unavailable: {error}"))?
    {
        KeyStatus::Trusted { .. } => {}
        KeyStatus::Untrusted { .. } => {
            return Err(format!(
                "plugin package `{}` has an untrusted publisher",
                installed.plugin_id
            ));
        }
        KeyStatus::Changed { .. } => {
            return Err(format!(
                "plugin package `{}` has a changed publisher key",
                installed.plugin_id
            ));
        }
    }
    let report = packages
        .inspect_candidate(
            &installed.package_path,
            publisher_key,
            EmulationPolicy::RequireNative,
        )
        .await
        .map_err(|error| {
            format!(
                "plugin package `{}` is invalid: {error}",
                installed.plugin_id
            )
        })?;
    report.selected_target().map_err(|error| {
        format!(
            "plugin package `{}` is incompatible: {error}",
            installed.plugin_id
        )
    })?;

    let backend = PluginBackend {
        plugin_id: installed.plugin_id.clone(),
        provider_id: provider_id.clone(),
        discovery: true,
        catalog: false,
        counter: false,
        actions: false,
        achievements: false,
    };
    let synthetic = synthetic_manifest(&provider_id, &package.name, platform, &backend.plugin_id);
    let loaded = provider::load_with_plugins(
        &[("community-provider.toml", synthetic.as_str())],
        platform,
        std::slice::from_ref(&backend),
    );
    let provider = loaded.providers.into_iter().next().ok_or_else(|| {
        loaded
            .diagnostics
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    Ok(ComposedProvider {
        plugin_id: installed.plugin_id,
        provider_id,
        provider,
    })
}

async fn read_manifest(installed: &InstalledPluginVersion) -> Result<ValidatedManifest, String> {
    let root = installed.package_path.clone();
    tokio::task::spawn_blocking(move || read_manifest_blocking(&root))
        .await
        .map_err(|_| "plugin manifest worker failed".to_owned())?
}

/// Reads the same bounded, non-symlink manifest the runtime composition reads.
fn read_manifest_blocking(package_root: &Path) -> Result<ValidatedManifest, String> {
    let path = package_root.join("plugin.json");
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("plugin manifest unavailable: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 4 * 1024 * 1024
    {
        return Err("plugin manifest is not a bounded regular file".to_owned());
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("plugin manifest unavailable: {error}"))?;
    super::package::Manifest::parse_json(&bytes)
        .map_err(|error| format!("plugin manifest is invalid: {error}"))
}

fn synthetic_manifest(
    provider_id: &str,
    name: &str,
    platform: Platform,
    plugin_id: &str,
) -> String {
    format!(
        "id = {provider_id:?}\nname = {name:?}\nplatforms = [{platform:?}]\nsource = \"community\"\n\n[scan]\nplugin = {plugin_id:?}\n",
        platform = platform_name(platform)
    )
}

fn platform_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Windows => "windows",
        Platform::Linux => "linux",
    }
}

#[cfg(test)]
mod tests;
