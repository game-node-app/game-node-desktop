//! Loading, validating and running provider manifests.
//!
//! The loader takes `(file name, text)` pairs so `core` never reaches outside its crate for
//! files: the app embeds `providers/*.toml` at build time and passes them in, and tests pass
//! manifests inline. Each manifest is validated on its own; an invalid one produces a
//! [`Diagnostic`] and the others still load.

use std::collections::BTreeSet;
use std::fmt;

use url::Url;

use super::manifest::{Manifest, Platform, RootSpec};
use super::record::{RawRecord, Record, finish_records};
use super::template::{FillError, Template};
use crate::ports::{HostFiles, RootResolver};

/// What a scanner gets: file access and the resolved root folders to look in.
pub struct ScanInput<'a> {
    pub files: &'a dyn HostFiles,
    pub roots: &'a [String],
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ScanError {
    #[error("the scan failed: {0}")]
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CounterValue {
    pub seconds: i64,
    /// `None` when the store has no date for the total, which is different from a known zero.
    pub last_played: Option<i64>,
}
pub type CounterFn = fn(&ScanInput<'_>) -> std::collections::BTreeMap<String, CounterValue>;

pub fn counter(_name: &str) -> Option<CounterFn> {
    None
}

pub type ScanFn = fn(&ScanInput<'_>) -> Result<Vec<RawRecord>, ScanError>;

/// Host-approved plugin operations. Core sees only domain capabilities; it does
/// not import the transport SDK or know how to start a provider executable.
#[derive(Clone, Debug)]
pub struct PluginBackend {
    pub plugin_id: String,
    pub provider_id: String,
    pub discovery: bool,
    pub catalog: bool,
    pub counter: bool,
    pub actions: bool,
    pub achievements: bool,
}

fn plugin_scan(_: &ScanInput<'_>) -> Result<Vec<RawRecord>, ScanError> {
    Err(ScanError::Failed(
        "this provider requires its registered plugin".into(),
    ))
}

/// The scanners built into the application, by manifest name.
pub fn native_scanner(name: &str) -> Option<ScanFn> {
    match name {
        "manual" => Some(|_| Ok(Vec::new())),
        _ => None,
    }
}

/// Why a manifest was skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// The manifest file name.
    pub file: String,
    /// The provider id, when it could be read.
    pub provider: Option<String>,
    pub problem: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.provider {
            Some(provider) => write!(f, "{} (provider `{provider}`): {}", self.file, self.problem),
            None => write!(f, "{}: {}", self.file, self.problem),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Root {
    Path(String),
    Registry { key: String, value: String },
}

/// Why Play or Install cannot produce a URI for a record.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ActionError {
    /// The manifest declares no such action.
    #[error("the provider does not support this action")]
    Unsupported,
    #[error(transparent)]
    Fill(#[from] FillError),
}

/// A validated provider, ready to scan and to build launch and install URIs.
#[derive(Clone)]
pub struct Provider {
    manifest: Manifest,
    scanner: ScanFn,
    launch: Option<Template>,
    install: Option<Template>,
    roots: Vec<(Platform, Root)>,
}

impl fmt::Debug for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Provider")
            .field("id", &self.manifest.id)
            .finish_non_exhaustive()
    }
}

impl Provider {
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    pub fn source(&self) -> &str {
        &self.manifest.source
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The GameNode external-game category used to match this provider's ids, if any.
    pub fn external_category(&self) -> Option<u32> {
        self.manifest.matching.as_ref().map(|m| m.external_category)
    }

    /// The record field that holds this provider's external id, when the manifest names one.
    pub fn uid_field(&self) -> Option<&str> {
        self.manifest
            .matching
            .as_ref()
            .and_then(|matching| matching.uid_field.as_deref())
            .filter(|field| !field.trim().is_empty())
    }

    /// Whether the provider can read an owned/family catalog beyond installed games.
    pub fn has_catalog(&self) -> bool {
        self.manifest.catalog.is_some()
    }

    /// The built-in catalog backend the manifest names, if any.
    pub fn catalog_backend(&self) -> Option<&str> {
        self.manifest
            .catalog
            .as_ref()
            .map(|catalog| catalog.plugin.as_deref().unwrap_or(catalog.native.as_str()))
    }

    pub fn discovery_plugin(&self) -> Option<&str> {
        self.manifest.scan.plugin.as_deref()
    }

    pub fn counter_plugin(&self) -> Option<&str> {
        self.manifest
            .playtime
            .as_ref()
            .and_then(|p| p.plugin.as_deref())
    }

    /// The plugin that reads this provider's achievements, if the manifest declares one.
    pub fn achievement_plugin(&self) -> Option<&str> {
        self.manifest
            .achievements
            .as_ref()
            .map(|achievements| achievements.plugin.as_str())
    }

    pub fn is_manual_executable(&self) -> bool {
        self.id() == "manual"
            && matches!(&self.manifest.launch, Some(super::manifest::UriAction::Executable { executable }) if executable == "manual")
    }
    pub fn has_launch(&self) -> bool {
        self.launch.is_some() || self.is_manual_executable()
    }

    pub fn has_install(&self) -> bool {
        self.install.is_some()
    }

    /// The URI that plays the game. Fails if the provider has no launch template or the record
    /// lacks a field the template needs.
    pub fn launch_uri(&self, record: &Record) -> Result<Url, ActionError> {
        Ok(self
            .launch
            .as_ref()
            .ok_or(ActionError::Unsupported)?
            .fill(record)?)
    }

    /// The URI that installs the game through the store client.
    pub fn install_uri(&self, record: &Record) -> Result<Url, ActionError> {
        Ok(self
            .install
            .as_ref()
            .ok_or(ActionError::Unsupported)?
            .fill(record)?)
    }

    /// Whether the launch template can be filled from the record.
    pub fn can_launch(&self, record: &Record) -> bool {
        (self.is_manual_executable() && record.field("available") == Some("true"))
            || self.launch.as_ref().is_some_and(|t| t.applies_to(record))
    }

    pub fn can_install(&self, record: &Record) -> bool {
        self.install.as_ref().is_some_and(|t| t.applies_to(record))
    }

    pub fn resolve_roots(&self, platform: Platform, resolver: &dyn RootResolver) -> Vec<String> {
        self.roots
            .iter()
            .filter(|(root_platform, _)| *root_platform == platform)
            .filter_map(|(_, root)| match root {
                Root::Path(path) => resolver.expand_home(path),
                Root::Registry { key, value } => resolver.registry_string(key, value),
            })
            .filter(|path| !path.trim().is_empty())
            .collect()
    }

    /// Runs the provider's scanner and returns complete, de-duplicated records.
    pub fn scan(
        &self,
        platform: Platform,
        files: &dyn HostFiles,
        resolver: &dyn RootResolver,
    ) -> Result<Vec<Record>, ScanError> {
        let roots = self.resolve_roots(platform, resolver);
        let raw = (self.scanner)(&ScanInput {
            files,
            roots: &roots,
        })?;
        Ok(finish_records(self.id(), raw, &self.manifest.exclude_ids))
    }
}

/// The providers that loaded plus a diagnostic for each manifest that did not.
#[derive(Clone, Debug, Default)]
pub struct LoadedProviders {
    pub providers: Vec<Provider>,
    pub diagnostics: Vec<Diagnostic>,
}

impl LoadedProviders {
    pub fn get(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|provider| provider.id() == id)
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

fn convert_roots(manifest: &Manifest) -> Result<Vec<(Platform, Root)>, String> {
    let mut roots = Vec::new();
    for platform in [Platform::Windows, Platform::Linux] {
        for spec in manifest.roots.for_platform(platform) {
            roots.push((platform, convert_root(platform, spec)?));
        }
    }
    Ok(roots)
}

fn convert_root(platform: Platform, spec: &RootSpec) -> Result<Root, String> {
    match (&spec.path, &spec.registry, &spec.value) {
        (Some(path), None, None) if !path.trim().is_empty() => Ok(Root::Path(path.clone())),
        (None, Some(key), Some(value)) if !value.trim().is_empty() => {
            if platform != Platform::Windows {
                return Err("a registry root is only valid under `roots.windows`".to_owned());
            }
            // The hive stays in the key so resolvers can read either a per-user or a
            // machine-wide value. Steam's existing `HKCU\Software\Valve\Steam` is unchanged.
            let recognized = key
                .split_once('\\')
                .is_some_and(|(hive, rest)| !rest.is_empty() && matches!(hive, "HKCU" | "HKLM"));
            if !recognized {
                return Err(format!(
                    "registry root `{key}` must start with `HKCU\\` or `HKLM\\`"
                ));
            }
            Ok(Root::Registry {
                key: key.clone(),
                value: value.clone(),
            })
        }
        _ => Err("a root needs either `path`, or `registry` together with `value`".to_owned()),
    }
}

fn template(
    action: &Option<super::manifest::UriAction>,
    what: &str,
) -> Result<Option<Template>, String> {
    action
        .as_ref()
        .and_then(|action| match action {
            super::manifest::UriAction::Uri { uri } => {
                Some(Template::parse(uri).map_err(|error| format!("{what}: {error}")))
            }
            _ => None,
        })
        .transpose()
}

/// Checks everything about a parsed manifest except duplicate ids.
fn validate(
    manifest: Manifest,
    plugins: &[PluginBackend],
    active: bool,
) -> Result<Provider, String> {
    if !valid_id(&manifest.id) {
        return Err(format!(
            "`id` must be non-empty lowercase letters, digits, `-` or `_`, not `{}`",
            manifest.id
        ));
    }
    if manifest.name.trim().is_empty() {
        return Err("`name` is empty".to_owned());
    }
    if manifest.source.trim().is_empty() {
        return Err("`source` is empty".to_owned());
    }
    if manifest.platforms.is_empty() {
        return Err("`platforms` is empty".to_owned());
    }
    // Foreign-platform manifests still receive structural validation, but do
    // not require a backend that the current host deliberately does not ship.
    let inactive_backend = PluginBackend {
        plugin_id: String::new(),
        provider_id: String::new(),
        discovery: true,
        catalog: true,
        counter: true,
        actions: true,
        achievements: true,
    };
    let plugin = |id: &str| -> Result<&PluginBackend, String> {
        if !active {
            return Ok(&inactive_backend);
        }
        plugins
            .iter()
            .find(|p| p.plugin_id == id && p.provider_id == manifest.id)
            .ok_or_else(|| "provider plugin is not registered for this provider".into())
    };
    let scanner: ScanFn = match &manifest.scan.plugin {
        Some(id) => {
            if !manifest.scan.native.is_empty() || !plugin(id)?.discovery {
                return Err("invalid or unauthorized plugin discovery declaration".into());
            }
            plugin_scan
        }
        None => native_scanner(&manifest.scan.native)
            .ok_or_else(|| format!("unknown native scanner `{}`", manifest.scan.native))?,
    };
    if let Some(catalog) = &manifest.catalog {
        match &catalog.plugin {
            Some(id) if catalog.native.is_empty() && plugin(id)?.catalog => {}
            Some(_) => return Err("invalid or unauthorized plugin catalog declaration".into()),
            None => return Err(format!("unknown catalog backend `{}`", catalog.native)),
        }
    }
    if let Some(playtime) = &manifest.playtime {
        match &playtime.plugin {
            Some(id) if playtime.counter.is_empty() && plugin(id)?.counter => {}
            Some(_) => return Err("invalid or unauthorized plugin counter declaration".into()),
            None if counter(&playtime.counter).is_some() => {}
            None => return Err(format!("unknown playtime counter `{}`", playtime.counter)),
        }
    }
    if let Some(achievements) = &manifest.achievements
        && (achievements.plugin.is_empty() || !plugin(&achievements.plugin)?.achievements)
    {
        return Err("invalid or unauthorized achievement backend declaration".into());
    }
    if let Some(id) = &manifest.scan.plugin
        && (manifest.launch.is_some() || manifest.install.is_some())
        && !plugin(id)?.actions
    {
        return Err("provider plugin actions are not authorized".into());
    }
    let executable = matches!(
        manifest.launch,
        Some(super::manifest::UriAction::Executable { .. })
    );
    if executable
        && (manifest.id != "manual"
            || manifest.source != "manual"
            || manifest.scan.native != "manual"
            || manifest.platforms != [Platform::Windows]
            || manifest.playtime.is_some()
            || manifest.install.is_some()
            || !matches!(&manifest.launch, Some(super::manifest::UriAction::Executable { executable }) if executable == "manual"))
    {
        return Err("executable actions are restricted to the built-in manual provider".into());
    }
    if matches!(
        manifest.install,
        Some(super::manifest::UriAction::Executable { .. })
    ) {
        return Err("executable install actions are unsupported".into());
    }
    let launch = template(&manifest.launch, "launch")?;
    let install = template(&manifest.install, "install")?;
    if manifest.exclude_ids.iter().any(|id| id.trim().is_empty()) {
        return Err("`exclude_ids` contains an empty id".to_owned());
    }
    let roots = convert_roots(&manifest)?;
    Ok(Provider {
        manifest,
        scanner,
        launch,
        install,
        roots,
    })
}

/// Loads manifests for `platform`. Sources are processed in file-name order so results do not
/// depend on how the build listed them; on a duplicate id the first file wins.
pub fn load(sources: &[(&str, &str)], platform: Platform) -> LoadedProviders {
    load_with_plugins(sources, platform, &[])
}

pub fn load_with_plugins(
    sources: &[(&str, &str)],
    platform: Platform,
    plugins: &[PluginBackend],
) -> LoadedProviders {
    let mut ordered: Vec<&(&str, &str)> = sources.iter().collect();
    ordered.sort_by_key(|(file, _)| *file);

    let mut loaded = LoadedProviders::default();
    let mut ids = BTreeSet::new();
    for (file, text) in ordered {
        let diagnostic = |provider: Option<String>, problem: String| Diagnostic {
            file: (*file).to_owned(),
            provider,
            problem,
        };

        let table: toml::Table = match toml::from_str(text) {
            Ok(table) => table,
            Err(error) => {
                loaded
                    .diagnostics
                    .push(diagnostic(None, error.message().to_owned()));
                continue;
            }
        };
        let id = table
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_owned);
        let manifest: Manifest = match table.try_into() {
            Ok(manifest) => manifest,
            Err(error) => {
                loaded
                    .diagnostics
                    .push(diagnostic(id, error.message().to_owned()));
                continue;
            }
        };
        let active = manifest.platforms.contains(&platform);
        let provider = match validate(manifest, plugins, active) {
            Ok(provider) => provider,
            Err(problem) => {
                loaded.diagnostics.push(diagnostic(id, problem));
                continue;
            }
        };
        if !active {
            continue;
        }
        if !ids.insert(provider.id().to_owned()) {
            loaded.diagnostics.push(diagnostic(
                Some(provider.id().to_owned()),
                "duplicate provider id".to_owned(),
            ));
            continue;
        }
        loaded.providers.push(provider);
    }
    for diagnostic in &loaded.diagnostics {
        tracing::warn!(%diagnostic, "skipped an invalid provider manifest");
    }
    loaded
}

#[cfg(test)]
pub(crate) mod tests;
