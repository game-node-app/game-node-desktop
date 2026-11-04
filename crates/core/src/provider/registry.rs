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
use super::steam;
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

pub type CounterFn =
    fn(&ScanInput<'_>) -> std::collections::BTreeMap<String, steam::counter::CounterValue>;

pub fn counter(name: &str) -> Option<CounterFn> {
    match name {
        "steam" => Some(steam::counter::read),
        _ => None,
    }
}

pub type ScanFn = fn(&ScanInput<'_>) -> Result<Vec<RawRecord>, ScanError>;

/// The scanners built into the application, by manifest name.
pub fn native_scanner(name: &str) -> Option<ScanFn> {
    match name {
        "steam" => Some(steam::scan),
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

    pub fn has_launch(&self) -> bool {
        self.launch.is_some()
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
        self.launch.as_ref().is_some_and(|t| t.applies_to(record))
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
            let Some(relative) = key.strip_prefix("HKCU\\").filter(|rest| !rest.is_empty()) else {
                return Err(format!("registry root `{key}` must start with `HKCU\\`"));
            };
            Ok(Root::Registry {
                key: relative.to_owned(),
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
        .map(|action| Template::parse(&action.uri).map_err(|error| format!("{what}: {error}")))
        .transpose()
}

/// Checks everything about a parsed manifest except duplicate ids.
fn validate(manifest: Manifest) -> Result<Provider, String> {
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
    let scanner = native_scanner(&manifest.scan.native)
        .ok_or_else(|| format!("unknown native scanner `{}`", manifest.scan.native))?;
    if let Some(playtime) = &manifest.playtime
        && counter(&playtime.counter).is_none()
    {
        return Err(format!("unknown playtime counter `{}`", playtime.counter));
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
        let provider = match validate(manifest) {
            Ok(provider) => provider,
            Err(problem) => {
                loaded.diagnostics.push(diagnostic(id, problem));
                continue;
            }
        };
        if !provider.manifest.platforms.contains(&platform) {
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
