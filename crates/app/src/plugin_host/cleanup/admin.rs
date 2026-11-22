//! Host-only physical storage accounting for plugin data.
//!
//! This is host administration, not a plugin route: nothing here is served over `ResourceHost`
//! and no plugin RPC can reach it. It reports the host-managed physical files, their measured
//! allocation, the quota-charged logical bytes of one plugin and the effective limits, so the
//! settings surface can explain where plugin data lives and what cleanup would free. Paths are
//! host UI only; every plugin-facing usage reply keeps reporting logical values alone.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gamenode_platform::plugin_shared_data::{
    FILE_NAME as SHARED_POOL_FILE, MAX_OWNER_LOGICAL_BYTES, OwnerScope,
};
use gamenode_platform::plugin_storage::namespace_store_file;
use tonic::Status;

use super::{effective_private_max_bytes, unix_now};
use crate::plugin_host::runtime::ThirdPartyPluginLifecycle;

/// One physical file holding plugin data of the active profile.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStorageLocation {
    pub plugin_id: String,
    /// `durable_shared` or `private_persistence`.
    pub kind: String,
    /// The host-managed file on disk. Shown only in the host UI.
    pub path: String,
    /// `namespace` when the file belongs to this plugin/profile alone, `pool` when every
    /// third-party producer shares the same SQLite file.
    pub scope: String,
    /// Measured bytes of the database file plus its write-ahead log and shared-memory sidecars.
    pub file_bytes: u64,
    /// Quota-charged bytes of this plugin's live records.
    pub logical_bytes: u64,
    pub records: u64,
    pub protected_records: u64,
    pub archived_records: u64,
    pub oldest_used_at: Option<i64>,
    /// Effective byte limit the automatic retention path applies.
    pub max_bytes: Option<u64>,
    pub max_age_seconds: Option<u64>,
}

/// The durable pool file, accounted once because every producer shares it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStoragePool {
    pub path: String,
    /// Measured bytes of the pool file and its SQLite sidecars.
    pub file_bytes: u64,
    /// Live logical bytes of every accounted producer in the pool.
    pub logical_bytes: u64,
    /// `file_bytes` minus the accounted logical bytes; SQLite metadata and free pages.
    pub overhead_bytes: u64,
    pub owners: u64,
    pub max_owner_bytes: u64,
}

/// Host-only accounting of the active profile's plugin data.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStorageAccounting {
    pub profile_id: String,
    /// One entry per plugin and store that holds data; no value or raw owner key.
    pub locations: Vec<PluginStorageLocation>,
    /// `None` when this host has no durable pool file.
    pub pool: Option<PluginStoragePool>,
    pub private_root_bytes: u64,
}

impl ThirdPartyPluginLifecycle {
    /// Physical accounting of the active profile's plugin data, measured on a blocking worker.
    pub async fn storage_locations(&self) -> Result<PluginStorageAccounting, Status> {
        let now = unix_now()?;
        self.storage_locations_at(now).await
    }

    pub(crate) async fn storage_locations_at(
        &self,
        now: i64,
    ) -> Result<PluginStorageAccounting, Status> {
        let api = self.host_api();
        let profile = api.profiles().require()?;
        let profile_id = profile.profile_id().to_owned();
        let profile_epoch = profile.epoch();
        let storage = self.private_storage();
        let pool = api.shared_pool();
        let pool_file = pool.data_dir().map(|dir| dir.join(SHARED_POOL_FILE));

        let mut identities: BTreeMap<(String, String), ()> = BTreeMap::new();
        for provider_id in api.third_party_provider_ids() {
            if let Some((plugin_id, publisher)) = api.third_party_identity(&provider_id) {
                identities.insert((plugin_id, publisher), ());
            }
        }

        let mut locations: Vec<PluginStorageLocation> = Vec::new();
        let mut pool_logical_bytes = 0_u64;
        let mut pool_owners = 0_u64;
        for (plugin_id, publisher_fingerprint) in identities.into_keys() {
            let resolver = api
                .profiles()
                .resolver(&plugin_id, &publisher_fingerprint, None)?;
            let owner_key = *resolver.storage_owner().key();
            let owner = OwnerScope::new(owner_key, profile_epoch)
                .map_err(|_| Status::failed_precondition("plugin profile is invalid"))?;

            if let Some(usage) = pool.usage(owner, now).await?
                && (usage.records > 0 || usage.archived_records > 0)
            {
                pool_logical_bytes = pool_logical_bytes.saturating_add(usage.logical_bytes);
                pool_owners = pool_owners.saturating_add(1);
                locations.push(PluginStorageLocation {
                    plugin_id: plugin_id.clone(),
                    kind: "durable_shared".to_owned(),
                    path: pool_file.as_deref().map(display_path).unwrap_or_default(),
                    scope: "pool".to_owned(),
                    file_bytes: 0,
                    logical_bytes: usage.logical_bytes,
                    records: usage.records,
                    protected_records: usage.protected_records,
                    archived_records: usage.archived_records,
                    oldest_used_at: usage.oldest_updated_at,
                    max_bytes: Some(usage.max_bytes),
                    max_age_seconds: usage.max_age_seconds,
                });
            }

            let Some(storage) = storage.as_ref() else {
                continue;
            };
            let Some(usage) = storage.private_usage(&resolver, profile_epoch).await? else {
                continue;
            };
            let records: u64 = usage
                .categories
                .iter()
                .map(|category| category.records)
                .sum();
            let protected: u64 = usage
                .categories
                .iter()
                .map(|category| category.protected_records)
                .sum();
            if records == 0 && usage.archived_records == 0 {
                continue;
            }
            let identity = resolver.identity();
            let path = storage.root().map(|root| {
                namespace_store_file(
                    root,
                    &identity.publisher_fingerprint,
                    &identity.plugin_id,
                    &identity.profile_id,
                )
            });
            let policy_max = usage.retention.map_or(0, |policy| policy.max_bytes);
            locations.push(PluginStorageLocation {
                plugin_id,
                kind: "private_persistence".to_owned(),
                path: path.as_deref().map(display_path).unwrap_or_default(),
                scope: "namespace".to_owned(),
                file_bytes: 0,
                logical_bytes: usage.logical_bytes,
                records,
                protected_records: protected,
                archived_records: usage.archived_records,
                oldest_used_at: usage.oldest_used_at,
                max_bytes: Some(effective_private_max_bytes(policy_max)),
                max_age_seconds: usage.retention.and_then(|policy| {
                    (policy.max_age_seconds > 0).then_some(policy.max_age_seconds)
                }),
            });
        }

        // One measuring pass over the distinct files; a missing file measures as zero.
        let mut measured: BTreeMap<PathBuf, u64> = BTreeMap::new();
        for location in &locations {
            if !location.path.is_empty() {
                measured.entry(PathBuf::from(&location.path)).or_default();
            }
        }
        let paths: Vec<PathBuf> = measured.keys().cloned().collect();
        if !paths.is_empty() {
            let sizes = tokio::task::spawn_blocking(move || {
                paths
                    .into_iter()
                    .map(|path| {
                        let bytes = physical_file_bytes(&path);
                        (path, bytes)
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .await
            .map_err(|_| {
                Status::unavailable("the plugin storage accounting worker is unavailable")
            })?;
            measured = sizes;
        }
        let mut private_root_bytes = 0_u64;
        for location in &mut locations {
            let bytes = measured
                .get(Path::new(&location.path))
                .copied()
                .unwrap_or(0);
            location.file_bytes = bytes;
            if location.scope == "namespace" {
                private_root_bytes = private_root_bytes.saturating_add(bytes);
            }
        }

        let pool = pool_file.map(|path| {
            let file_bytes = measured.get(&path).copied().unwrap_or(0);
            PluginStoragePool {
                path: display_path(&path),
                file_bytes,
                logical_bytes: pool_logical_bytes,
                overhead_bytes: file_bytes.saturating_sub(pool_logical_bytes),
                owners: pool_owners,
                max_owner_bytes: MAX_OWNER_LOGICAL_BYTES,
            }
        });

        Ok(PluginStorageAccounting {
            profile_id,
            locations,
            pool,
            private_root_bytes,
        })
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The bytes a SQLite database occupies on disk, including its `-wal` and `-shm` sidecars.
fn physical_file_bytes(path: &Path) -> u64 {
    let mut total = std::fs::metadata(path).map_or(0, |metadata| metadata.len());
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = path.as_os_str().to_owned();
        sidecar.push(suffix);
        total = total.saturating_add(
            std::fs::metadata(PathBuf::from(sidecar)).map_or(0, |metadata| metadata.len()),
        );
    }
    total
}
