//! Origin scopes and limits for the plugins' `Network.Fetch`.
//!
//! `network.fetch` is only half of the authority: the user also approves the exact HTTPS origins
//! (scheme, host and port) a plugin may reach, and nothing is reachable until one is approved.
//! The origins come from an [`OriginScopes`] source that is asked afresh for every fetch and for
//! every redirect hop, so a revoked or narrowed approval stops a fetch already under way. The
//! source is connected with the plugin activation code (the permission store); until it is, no
//! origin is approved and every fetch is denied.
//!
//! The fetching itself, with its address, redirect, size and deadline rules, is
//! [`gamenode_platform::plugin_network`]; this module bounds how many run at once.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use gamenode_platform::plugin_network::{FetchClient, FetchError, Request, Response, origin_of};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tonic::Status;
use url::Url;

use super::grants::PluginIdentity;

/// Fetches one plugin may have in flight.
pub const PER_PLUGIN_FETCHES: usize = 4;
/// Fetches in flight for all plugins together.
pub const GLOBAL_FETCHES: usize = 16;

/// The check a fetch makes before every origin it is about to contact.
pub type AllowOrigin = Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// Whatever performs a fetch. The real one is [`FetchClient`]; tests supply their own.
#[async_trait]
pub trait Fetcher: Send + Sync {
    /// `allow` is asked, with the exact origin, before the first request and before every
    /// redirect is followed.
    async fn fetch(&self, request: Request, allow: AllowOrigin) -> Result<Response, FetchError>;
}

#[async_trait]
impl Fetcher for FetchClient {
    async fn fetch(&self, request: Request, allow: AllowOrigin) -> Result<Response, FetchError> {
        FetchClient::fetch(self, request, move |origin: &str| allow(origin)).await
    }
}

/// The exact HTTPS origins the user has approved for a plugin, as they stand now.
pub trait OriginScopes: Send + Sync {
    fn approved(&self, identity: &PluginIdentity) -> Vec<String>;
}

/// An origin as the fetch compares it: `https://host` or `https://host:port`, lowercase, with
/// the default port left out. Anything that is not exactly an origin (a path, a query, a
/// scheme other than https, credentials) is not one.
fn canonical_origin(text: &str) -> Option<String> {
    let url = Url::parse(text).ok()?;
    let plain = url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && matches!(url.path(), "" | "/");
    plain.then(|| origin_of(&url)).flatten()
}

/// Origin scopes and concurrency limits shared by every plugin session.
pub struct NetworkAccess {
    fetcher: Option<Arc<dyn Fetcher>>,
    scopes: Mutex<Option<Arc<dyn OriginScopes>>>,
    global: Arc<Semaphore>,
    per_plugin: Mutex<HashMap<[u8; 32], Arc<Semaphore>>>,
}

impl Default for NetworkAccess {
    /// The real client; fetching is unavailable if it cannot be built.
    fn default() -> Self {
        match FetchClient::new() {
            Ok(client) => Self::new(Arc::new(client)),
            Err(error) => {
                tracing::warn!(%error, "plugin network fetch is unavailable");
                Self::unavailable()
            }
        }
    }
}

impl NetworkAccess {
    pub fn new(fetcher: Arc<dyn Fetcher>) -> Self {
        Self {
            fetcher: Some(fetcher),
            ..Self::unavailable()
        }
    }

    fn unavailable() -> Self {
        Self {
            fetcher: None,
            scopes: Mutex::default(),
            global: Arc::new(Semaphore::new(GLOBAL_FETCHES)),
            per_plugin: Mutex::default(),
        }
    }

    /// Connects the source of approved origins.
    pub fn set_scopes(&self, scopes: Arc<dyn OriginScopes>) {
        *self.scopes.lock().unwrap_or_else(|p| p.into_inner()) = Some(scopes);
    }

    pub(crate) fn fetcher(&self) -> Option<Arc<dyn Fetcher>> {
        self.fetcher.clone()
    }

    /// The origins approved for `identity` now. Empty when no source is connected.
    pub(crate) fn approved_origins(&self, identity: &PluginIdentity) -> BTreeSet<String> {
        let scopes = self
            .scopes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        scopes
            .map(|scopes| {
                scopes
                    .approved(identity)
                    .iter()
                    .filter_map(|text| canonical_origin(text))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// One slot of the plugin's own allowance and one of the global one, or
    /// `RESOURCE_EXHAUSTED`. Fetches are not queued: a plugin that is over its allowance is told
    /// so and may retry.
    pub(crate) fn permits(&self, owner: [u8; 32]) -> Result<Permits, Status> {
        let own = self
            .per_plugin
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(owner)
            .or_insert_with(|| Arc::new(Semaphore::new(PER_PLUGIN_FETCHES)))
            .clone();
        let busy =
            || Status::resource_exhausted("too many fetches are already running; retry later");
        let own = own.try_acquire_owned().map_err(|_| busy())?;
        let global = self
            .global
            .clone()
            .try_acquire_owned()
            .map_err(|_| busy())?;
        Ok(Permits {
            _own: own,
            _global: global,
        })
    }
}

/// Holds a fetch's slots until dropped.
pub(crate) struct Permits {
    _own: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

#[cfg(all(test, unix))]
mod tests;
