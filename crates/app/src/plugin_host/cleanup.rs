//! Reviewed host-wide cleanup of plugin storage.
//!
//! This is host administration, not a plugin route: nothing here is served over `ResourceHost`
//! and no plugin RPC can reach it. A review enumerates non-empty producers and private stores of
//! the active account profile with their logical sizes, ages and effective limits. Applying the
//! review requires the exact single-use token and an explicit confirmation; only the selected
//! owner candidates are pruned, through the same per-namespace retention paths the automatic
//! cleanup uses. User-protected and archived rows stay, another owner is never crossed, and a
//! partial failure is reported per candidate with the measured reclaimed space.

use std::collections::BTreeMap;
use std::sync::Mutex;

use gamenode_platform::plugin_shared_data::OwnerScope;
use gamenode_platform::plugin_storage::QUOTA_BYTES;
use sha2::{Digest, Sha256};
use tonic::Status;

use super::runtime::ThirdPartyPluginLifecycle;

mod admin;
pub use admin::{PluginStorageAccounting, PluginStorageLocation, PluginStoragePool};

/// The store a candidate belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CleanupKind {
    DurableShared,
    PrivatePersistence,
}

impl CleanupKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DurableShared => "durable_shared",
            Self::PrivatePersistence => "private_persistence",
        }
    }
}

/// One owner's non-empty dataset or private store, as presented for review. Contains no value,
/// physical path or raw owner key.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupCandidate {
    /// Opaque host id binding the kind and the owner key. It cannot be used to address a path.
    pub candidate_id: String,
    /// `durable_shared` or `private_persistence`.
    pub kind: String,
    pub plugin_id: String,
    pub profile_id: String,
    pub records: u64,
    pub protected_records: u64,
    pub archived_records: u64,
    pub logical_bytes: u64,
    /// Namespace- or pool-wide SQLite page estimate, never a per-owner figure.
    pub physical_bytes_estimate: u64,
    pub oldest_updated_at: Option<i64>,
    /// Effective byte limit the cleanup will apply. `None` means only the expiry/age path.
    pub max_bytes: Option<u64>,
    pub max_age_seconds: Option<u64>,
}

/// The review the user must confirm. The token is single-use and bound to the active profile and
/// the candidate snapshot.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupReview {
    pub token: String,
    pub generated_at: i64,
    pub candidates: Vec<StorageCleanupCandidate>,
}

/// One selected candidate's result.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupResult {
    pub candidate_id: String,
    pub kind: String,
    pub plugin_id: String,
    /// `completed` or `failed`.
    pub status: String,
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The measured result of one confirmed cleanup. The token cannot be reused.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageCleanupReport {
    pub token: String,
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    /// Protected records the reviewed candidates held and this cleanup kept.
    pub skipped_protected_records: u64,
    pub results: Vec<StorageCleanupResult>,
}

#[derive(Clone)]
struct ReviewedCandidate {
    candidate: StorageCleanupCandidate,
    owner_key: [u8; 32],
    publisher_fingerprint: String,
}

#[derive(Clone)]
struct IssuedReview {
    token: String,
    profile_id: String,
    profile_epoch: u64,
    candidates: Vec<ReviewedCandidate>,
}

/// Single-use cleanup reviews. Older reviews are dropped first, so a settings surface cannot grow
/// unbounded state.
#[derive(Default)]
pub struct CleanupReviews {
    issued: Mutex<Vec<IssuedReview>>,
}

const MAX_ISSUED_REVIEWS: usize = 8;

impl CleanupReviews {
    fn issue(&self, review: IssuedReview) {
        let mut issued = self.issued.lock().unwrap_or_else(|p| p.into_inner());
        issued.push(review);
        if issued.len() > MAX_ISSUED_REVIEWS {
            issued.remove(0);
        }
    }

    fn get(&self, token: &str) -> Option<IssuedReview> {
        self.issued
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|review| review.token == token)
            .cloned()
    }

    fn take(&self, token: &str) -> Option<IssuedReview> {
        let mut issued = self.issued.lock().unwrap_or_else(|p| p.into_inner());
        let index = issued.iter().position(|review| review.token == token)?;
        Some(issued.remove(index))
    }
}

fn candidate_id(kind: CleanupKind, owner_key: &[u8; 32]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"gamenode.cleanup.candidate.v1\0");
    digest.update(kind.as_str().as_bytes());
    digest.update(owner_key);
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn effective_private_max_bytes(policy_max: u64) -> u64 {
    if policy_max == 0 {
        QUOTA_BYTES
    } else {
        policy_max.min(QUOTA_BYTES)
    }
}

impl ThirdPartyPluginLifecycle {
    /// Enumerates cleanup candidates for the active profile. Reading never creates a pool or a
    /// private store file. The returned token must be confirmed before anything is deleted.
    pub async fn storage_cleanup_review(&self) -> Result<StorageCleanupReview, Status> {
        let now = unix_now()?;
        self.storage_cleanup_review_at(now).await
    }

    /// Applies one confirmed review. An unconfirmed call, an unknown token or a selection outside
    /// the review changes nothing. Each selected candidate is pruned on its own; a partial
    /// failure is reported and leaves the other candidates' results intact.
    pub async fn storage_cleanup_apply(
        &self,
        token: String,
        candidate_ids: Vec<String>,
        confirmed: bool,
    ) -> Result<StorageCleanupReport, Status> {
        let now = unix_now()?;
        self.storage_cleanup_apply_at(token, candidate_ids, confirmed, now)
            .await
    }

    pub(crate) async fn storage_cleanup_review_at(
        &self,
        now: i64,
    ) -> Result<StorageCleanupReview, Status> {
        let api = self.host_api();
        let profile = api.profiles().require()?;
        let profile_id = profile.profile_id().to_owned();
        let profile_epoch = profile.epoch();
        let mut identities: BTreeMap<(String, String), ()> = BTreeMap::new();
        for provider_id in api.third_party_provider_ids() {
            if let Some((plugin_id, publisher)) = api.third_party_identity(&provider_id) {
                identities.insert((plugin_id, publisher), ());
            }
        }
        let storage = self.private_storage();
        let mut candidates = Vec::new();
        for (plugin_id, publisher_fingerprint) in identities.into_keys() {
            let resolver = api
                .profiles()
                .resolver(&plugin_id, &publisher_fingerprint, None)?;
            let owner_key = *resolver.storage_owner().key();
            let owner = OwnerScope::new(owner_key, profile_epoch)
                .map_err(|_| Status::failed_precondition("plugin profile is invalid"))?;

            if let Some(usage) = api.shared_pool().usage(owner, now).await?
                && (usage.records > 0 || usage.archived_records > 0)
            {
                candidates.push(ReviewedCandidate {
                    candidate: StorageCleanupCandidate {
                        candidate_id: candidate_id(CleanupKind::DurableShared, &owner_key),
                        kind: CleanupKind::DurableShared.as_str().to_owned(),
                        plugin_id: plugin_id.clone(),
                        profile_id: profile_id.clone(),
                        records: usage.records,
                        protected_records: usage.protected_records,
                        archived_records: usage.archived_records,
                        logical_bytes: usage.logical_bytes,
                        physical_bytes_estimate: usage.physical_bytes_estimate,
                        oldest_updated_at: usage.oldest_updated_at,
                        max_bytes: Some(usage.max_bytes),
                        max_age_seconds: usage.max_age_seconds,
                    },
                    owner_key,
                    publisher_fingerprint: publisher_fingerprint.clone(),
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
            let policy_max = usage.retention.map_or(0, |policy| policy.max_bytes);
            candidates.push(ReviewedCandidate {
                candidate: StorageCleanupCandidate {
                    candidate_id: candidate_id(CleanupKind::PrivatePersistence, &owner_key),
                    kind: CleanupKind::PrivatePersistence.as_str().to_owned(),
                    plugin_id,
                    profile_id: profile_id.clone(),
                    records,
                    protected_records: protected,
                    archived_records: usage.archived_records,
                    logical_bytes: usage.logical_bytes,
                    physical_bytes_estimate: usage.physical_bytes_estimate,
                    oldest_updated_at: usage.oldest_used_at,
                    max_bytes: Some(effective_private_max_bytes(policy_max)),
                    max_age_seconds: usage.retention.and_then(|policy| {
                        (policy.max_age_seconds > 0).then_some(policy.max_age_seconds)
                    }),
                },
                owner_key,
                publisher_fingerprint,
            });
        }

        let token = gamenode_platform::random::nonce();
        self.cleanup_reviews().issue(IssuedReview {
            token: token.clone(),
            profile_id: profile_id.clone(),
            profile_epoch,
            candidates: candidates.clone(),
        });
        Ok(StorageCleanupReview {
            token,
            generated_at: now,
            candidates: candidates
                .into_iter()
                .map(|reviewed| reviewed.candidate)
                .collect(),
        })
    }

    /// Applies one reviewed candidate through the same per-namespace retention paths as the
    /// automatic cleanup, plus the archived reclaim when the review showed retained rows.
    async fn cleanup_candidate(
        &self,
        reviewed: &ReviewedCandidate,
        resolver: &super::namespace::ExternalNamespaceResolver,
        owner_key: [u8; 32],
        epoch: u64,
        now: i64,
    ) -> Result<(u64, u64), Status> {
        let candidate = &reviewed.candidate;
        if candidate.kind == CleanupKind::DurableShared.as_str() {
            let owner = OwnerScope::new(owner_key, epoch)
                .map_err(|_| Status::failed_precondition("plugin profile is invalid"))?;
            let mut removed = 0_u64;
            let mut reclaimed = 0_u64;
            if let Some(pruned) = self
                .host_api()
                .shared_pool()
                .prune_owner(owner, now)
                .await?
            {
                removed = pruned.removed_records;
                reclaimed = pruned.reclaimed_bytes;
            }
            if candidate.archived_records > 0
                && let Some(archived) = self
                    .host_api()
                    .shared_pool()
                    .reclaim_archived_owner(owner)
                    .await?
            {
                removed = removed.saturating_add(archived.removed_records);
                reclaimed = reclaimed.saturating_add(archived.reclaimed_bytes);
            }
            return Ok((removed, reclaimed));
        }

        let storage = self.private_storage().ok_or_else(|| {
            Status::failed_precondition("private plugin data is unavailable on this host")
        })?;
        let cleaned = storage.cleanup_persistence(resolver, epoch).await?;
        let mut removed = cleaned.removed_records;
        let mut reclaimed = cleaned.reclaimed_bytes;
        if candidate.archived_records > 0 {
            let archived = storage
                .reclaim_archived_persistence(resolver, epoch)
                .await?;
            removed = removed.saturating_add(archived.records);
            reclaimed = reclaimed.saturating_add(archived.logical_bytes);
        }
        Ok((removed, reclaimed))
    }

    pub(crate) async fn storage_cleanup_apply_at(
        &self,
        token: String,
        candidate_ids: Vec<String>,
        confirmed: bool,
        now: i64,
    ) -> Result<StorageCleanupReport, Status> {
        if !confirmed {
            return Err(Status::failed_precondition(
                "reviewed storage cleanup requires confirmation",
            ));
        }
        if candidate_ids.is_empty() {
            return Err(Status::invalid_argument(
                "select at least one cleanup candidate",
            ));
        }
        let mut unique = std::collections::BTreeSet::new();
        if candidate_ids.iter().any(|id| !unique.insert(id.clone())) {
            return Err(Status::invalid_argument("candidate ids must be unique"));
        }
        let review = self
            .cleanup_reviews()
            .get(&token)
            .ok_or_else(|| Status::failed_precondition("cleanup review expired or was used"))?;
        let profile = self.host_api().profiles().require()?;
        if profile.profile_id() != review.profile_id || profile.epoch() != review.profile_epoch {
            return Err(Status::failed_precondition(
                "the account profile changed; create a new cleanup review",
            ));
        }
        let by_id: BTreeMap<&str, &ReviewedCandidate> = review
            .candidates
            .iter()
            .map(|candidate| (candidate.candidate.candidate_id.as_str(), candidate))
            .collect();
        for id in &candidate_ids {
            if !by_id.contains_key(id.as_str()) {
                return Err(Status::invalid_argument(
                    "a selected candidate is not in this cleanup review",
                ));
            }
        }

        let mut results = Vec::with_capacity(candidate_ids.len());
        let mut removed_records = 0_u64;
        let mut reclaimed_bytes = 0_u64;
        for id in &candidate_ids {
            let reviewed = by_id[id.as_str()];
            let candidate = &reviewed.candidate;
            let registered_publisher = self
                .host_api()
                .third_party_provider_ids_for_plugin(&candidate.plugin_id)
                .into_iter()
                .find_map(|provider_id| {
                    self.host_api()
                        .third_party_identity(&provider_id)
                        .filter(|(plugin_id, _)| plugin_id == &candidate.plugin_id)
                        .map(|(_, publisher)| publisher)
                });
            let resolved = if registered_publisher.as_deref()
                != Some(reviewed.publisher_fingerprint.as_str())
            {
                Err(Status::failed_precondition(
                    "plugin identity changed since the cleanup review",
                ))
            } else {
                self.host_api().profiles().resolver(
                    &candidate.plugin_id,
                    &reviewed.publisher_fingerprint,
                    None,
                )
            };
            let outcome = match resolved {
                Ok(resolver) => {
                    let owner_key = *resolver.storage_owner().key();
                    if owner_key != reviewed.owner_key {
                        Err(Status::failed_precondition(
                            "plugin identity changed since the cleanup review",
                        ))
                    } else {
                        self.cleanup_candidate(
                            reviewed,
                            &resolver,
                            owner_key,
                            review.profile_epoch,
                            now,
                        )
                        .await
                    }
                }
                Err(error) => Err(error),
            };
            match outcome {
                Ok((removed, reclaimed)) => {
                    removed_records = removed_records.saturating_add(removed);
                    reclaimed_bytes = reclaimed_bytes.saturating_add(reclaimed);
                    results.push(StorageCleanupResult {
                        candidate_id: candidate.candidate_id.clone(),
                        kind: candidate.kind.clone(),
                        plugin_id: candidate.plugin_id.clone(),
                        status: "completed".to_owned(),
                        removed_records: removed,
                        reclaimed_bytes: reclaimed,
                        error: None,
                    });
                }
                Err(error) => results.push(StorageCleanupResult {
                    candidate_id: candidate.candidate_id.clone(),
                    kind: candidate.kind.clone(),
                    plugin_id: candidate.plugin_id.clone(),
                    status: "failed".to_owned(),
                    removed_records: 0,
                    reclaimed_bytes: 0,
                    error: Some(error.message().to_owned()),
                }),
            }
        }

        // Single use: even a partial failure consumes the reviewed snapshot and needs a new
        // review for a retry.
        self.cleanup_reviews().take(&token);
        let skipped_protected_records = candidate_ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()))
            .map(|candidate| candidate.candidate.protected_records)
            .sum();
        tracing::info!(
            %token,
            removed_records,
            reclaimed_bytes,
            skipped_protected_records,
            "reviewed host-wide plugin storage cleanup completed"
        );
        Ok(StorageCleanupReport {
            token,
            removed_records,
            reclaimed_bytes,
            skipped_protected_records,
            results,
        })
    }
}

fn unix_now() -> Result<i64, Status> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs().max(1) as i64)
        .map_err(|_| Status::failed_precondition("the system clock is before the Unix epoch"))
}

#[cfg(test)]
mod tests;
