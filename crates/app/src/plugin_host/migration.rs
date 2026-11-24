//! Reviewed migration of compatible data after a publisher key re-trust.
//!
//! A changed publisher fingerprint is a distinct security identity even after the user re-trusts
//! the new key. The owner namespace of the private store, the durable shared pool and the secret
//! slots all derive from that fingerprint, so old data stays isolated and inert. This module is the
//! host's administration of the one exception: an explicit, reviewed copy of *compatible* data
//! from the old identity to the new one.
//!
//! Nothing here is served over `ResourceHost`; there is no plugin RPC that can preview, apply,
//! cancel or roll back a migration, and a plugin never supplies an identity, owner key or path.
//! The old and new fingerprints come from the pinned package registry and the recorded trust
//! history. Secrets, activation grants, sharing policies and retention limits are never copied:
//! after a migration the new identity still needs its own current approvals. The source data is
//! never deleted, copies keep the original provenance and a reviewed rollback removes exactly the
//! copies a migration created.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use gamenode_platform::plugin_shared_data::{
    MigrationOutcome, MigrationRevertOutcome, OwnerScope, RecordMetadata,
};
use gamenode_platform::plugin_storage::{PrivateMigrationOutcome, PrivateMigrationRevert};
use gamenode_plugin_protocol::proto::ExtensionSchemaDescriptor;
use sha2::{Digest, Sha256};
use tonic::Status;

use super::namespace::ExternalNamespaceResolver;
use super::publisher_trust::PublisherTrustStore;
use super::runtime::ThirdPartyPluginLifecycle;
use super::sharing_policy::SharingClass;

/// Datasets the host itself serves. Their schema does not belong to the plugin package, so they
/// stay compatible regardless of the package's declared extension schemas.
const HOST_DATASETS: &[&str] = &[
    "storage.diagnostics.benchmark.v1",
    "storage.diagnostics.smart.v1",
    "computer.hardware.observations.v1",
    "game.presence.v1",
    "game.performance.history.v1",
];

const MAX_ISSUED_REVIEWS: usize = 8;
const MAX_REPORTED_SCHEMAS: usize = 16;

/// One dataset of the old identity as the preview presents it. Values, field names, record ids and
/// paths are never included.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherMigrationDataset {
    pub dataset_id: String,
    pub records: u64,
    pub eligible_records: u64,
    pub eligible_bytes: u64,
    pub conflict_records: u64,
    pub incompatible_schema_records: u64,
    /// Schema identifiers the new package no longer declares at a usable version, bounded.
    pub incompatible_schema_ids: Vec<String>,
    pub logical_bytes: u64,
}

/// The reviewed migration the user must confirm. The token is single-use and bound to the active
/// profile, its epoch and the exact old/new publisher identities.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherMigrationPreview {
    pub token: String,
    /// Stable identifier of this old/new/profile pair, used by a later reviewed rollback.
    pub migration_id: String,
    pub generated_at: i64,
    pub plugin_id: String,
    pub profile_id: String,
    pub profile_epoch: u64,
    pub source_publisher_fingerprint: String,
    pub target_publisher_fingerprint: String,
    /// Every other fingerprint the plugin was pinned to before the current key, oldest first.
    pub historical_publisher_fingerprints: Vec<String>,

    pub datasets: Vec<PublisherMigrationDataset>,
    pub shared_records: u64,
    pub shared_eligible_records: u64,
    pub shared_eligible_bytes: u64,
    pub shared_conflict_records: u64,
    pub shared_incompatible_schema_records: u64,
    pub shared_archived_records: u64,
    /// What the new identity already holds in the durable pool.
    pub shared_target_records: u64,
    pub shared_target_logical_bytes: u64,

    /// `false` when this host has no private data folder; the durable pool may still migrate.
    pub private_store_available: bool,
    pub private_records: u64,
    pub private_eligible_records: u64,
    pub private_eligible_bytes: u64,
    pub private_conflict_records: u64,
    pub private_expired_records: u64,
    pub private_archived_records: u64,
    pub private_protected_records: u64,
    pub private_target_records: u64,
    pub private_target_logical_bytes: u64,

    /// Credentials and approvals are never part of a migration.
    pub secrets_excluded: bool,
    pub grants_excluded: bool,
    /// The new identity needs a fresh sharing review before any consumer can read its data.
    pub sharing_review_required: bool,
    /// Retention limits are per owner and are not copied.
    pub retention_review_required: bool,
    pub can_migrate: bool,
}

/// One store's outcome in a migration or rollback report.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherMigrationStoreResult {
    /// `durable_shared` or `private_persistence`.
    pub store: String,
    /// `completed`, `failed` or `unavailable`.
    pub status: String,
    pub copied_records: u64,
    pub copied_bytes: u64,
    pub conflict_records: u64,
    pub skipped_over_quota_records: u64,
    pub expired_records: u64,
    pub archived_records: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The measured result of one confirmed migration. A partial failure is reported per store and
/// leaves the other store's result intact; the token cannot be reused after a failure.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherMigrationReport {
    pub token: String,
    pub migration_id: String,
    pub plugin_id: String,
    pub profile_id: String,
    pub source_publisher_fingerprint: String,
    pub target_publisher_fingerprint: String,
    pub results: Vec<PublisherMigrationStoreResult>,
    pub copied_records: u64,
    pub copied_bytes: u64,
    pub conflict_records: u64,
    pub skipped_over_quota_records: u64,
    pub incompatible_schema_records: u64,
    pub secrets_excluded: bool,
    pub grants_excluded: bool,
    pub sharing_review_required: bool,
    pub retention_review_required: bool,
}

/// One store's outcome in a rollback report.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherMigrationRollbackStoreResult {
    pub store: String,
    /// `completed`, `failed` or `unavailable`.
    pub status: String,
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    pub kept_protected_records: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The measured result of removing one migration's copies. The old identity's data stays.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherMigrationRollbackReport {
    pub migration_id: String,
    pub plugin_id: String,
    pub profile_id: String,
    pub source_publisher_fingerprint: String,
    pub target_publisher_fingerprint: String,
    pub results: Vec<PublisherMigrationRollbackStoreResult>,
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    pub kept_protected_records: u64,
}

#[derive(Clone)]
struct ReviewedMigration {
    token: String,
    migration_id: String,
    plugin_id: String,
    profile_id: String,
    profile_epoch: u64,
    source_fingerprint: String,
    target_fingerprint: String,
    source_owner_key: [u8; 32],
    target_owner_key: [u8; 32],
}

/// Single-use migration previews. Older previews are dropped first, so a settings surface cannot
/// grow unbounded state.
#[derive(Default)]
pub struct MigrationReviews {
    issued: Mutex<Vec<ReviewedMigration>>,
}

impl MigrationReviews {
    fn issue(&self, review: ReviewedMigration) {
        let mut issued = self.issued.lock().unwrap_or_else(|p| p.into_inner());
        issued.push(review);
        if issued.len() > MAX_ISSUED_REVIEWS {
            issued.remove(0);
        }
    }

    fn get(&self, token: &str) -> Option<ReviewedMigration> {
        self.issued
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .find(|review| review.token == token)
            .cloned()
    }

    fn take(&self, token: &str) -> Option<ReviewedMigration> {
        let mut issued = self.issued.lock().unwrap_or_else(|p| p.into_inner());
        let index = issued.iter().position(|review| review.token == token)?;
        Some(issued.remove(index))
    }
}

/// Stable identifier for one old/new/profile migration pair. It carries no key material: the
/// fingerprints are already the user-visible SHA-256 of a public key, and the digest only binds the
/// values.
fn migration_id(
    plugin_id: &str,
    source_fingerprint: &str,
    target_fingerprint: &str,
    profile_id: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(b"gamenode.publisher-migration.v1\0");
    for part in [
        plugin_id,
        source_fingerprint,
        target_fingerprint,
        profile_id,
    ] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Why a stored record cannot be carried to the new package.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SchemaVerdict {
    Compatible,
    ForeignOrUndeclared,
    OlderDeclaredVersion,
}

/// Decides whether the new package can still use one stored record's schema.
///
/// A schema the package declares is compatible when the declared version is at least the stored
/// one; a record at a newer schema version is not copied into code that cannot read it. An
/// undeclared schema is still compatible when the host serves that dataset itself or when the
/// schema belongs to the plugin's own reverse-domain namespace, because the package owns that
/// namespace. Anything else cannot be proven compatible and is reported instead of copied.
fn schema_verdict(
    plugin_id: &str,
    record: &RecordMetadata,
    declared: &[ExtensionSchemaDescriptor],
) -> SchemaVerdict {
    let mut matching = declared
        .iter()
        .filter(|descriptor| descriptor.schema_id == record.schema_id)
        .peekable();
    if matching.peek().is_some() {
        return if matching.any(|descriptor| descriptor.schema_version >= record.schema_version) {
            SchemaVerdict::Compatible
        } else {
            SchemaVerdict::OlderDeclaredVersion
        };
    }
    let owned = record.schema_id == plugin_id
        || record
            .schema_id
            .strip_prefix(plugin_id)
            .is_some_and(|rest| rest.starts_with('.'));
    if owned || HOST_DATASETS.contains(&record.schema_id.as_str()) {
        SchemaVerdict::Compatible
    } else {
        SchemaVerdict::ForeignOrUndeclared
    }
}

fn record_compatible(
    plugin_id: &str,
    record: &RecordMetadata,
    declared: &[ExtensionSchemaDescriptor],
) -> bool {
    SharingClass::from_name(&record.class).is_some()
        && schema_verdict(plugin_id, record, declared) == SchemaVerdict::Compatible
}

fn empty_migration_outcome() -> MigrationOutcome {
    MigrationOutcome {
        copied: Vec::new(),
        skipped: Vec::new(),
        pool_revision: 0,
    }
}

/// The old/new identities and the active package's declared schemas for one plugin.
struct ActivePublisher {
    source_fingerprint: String,
    target_fingerprint: String,
    prior_fingerprints: Vec<String>,
    declared_schemas: Vec<ExtensionSchemaDescriptor>,
}

impl ThirdPartyPluginLifecycle {
    /// Previews the data a re-trusted publisher identity could inherit from its predecessor.
    pub async fn publisher_migration_preview(
        &self,
        plugin_id: String,
    ) -> Result<PublisherMigrationPreview, Status> {
        let now = unix_now()?;
        self.publisher_migration_preview_at(plugin_id, now).await
    }

    /// Applies one confirmed, single-use migration preview. An unconfirmed or stale call changes
    /// nothing.
    pub async fn publisher_migration_apply(
        &self,
        token: String,
        expected_profile_id: String,
        confirmed: bool,
    ) -> Result<PublisherMigrationReport, Status> {
        let now = unix_now()?;
        self.publisher_migration_apply_at(token, expected_profile_id, confirmed, now)
            .await
    }

    /// Discards one preview. No data was changed by the preview, so a later apply cannot proceed
    /// with the cancelled token.
    pub async fn publisher_migration_cancel(&self, token: String) -> Result<(), Status> {
        self.migration_reviews()
            .take(&token)
            .map(|_| ())
            .ok_or_else(|| Status::failed_precondition("migration review expired or was used"))
    }

    /// Removes the copies one migration created from the new publisher identity. The old identity
    /// keeps its data. Only a pair this host recorded in the trust history can match.
    pub async fn publisher_migration_rollback(
        &self,
        plugin_id: String,
        reviewed_migration_id: String,
        expected_profile_id: String,
        confirmed: bool,
    ) -> Result<PublisherMigrationRollbackReport, Status> {
        if !confirmed {
            return Err(Status::failed_precondition(
                "rolling back a data migration requires confirmation",
            ));
        }
        let profile = self.host_api().profiles().require()?;
        if profile.profile_id() != expected_profile_id {
            return Err(Status::failed_precondition("plugin profile changed"));
        }
        let now = unix_now()?;
        let publisher = self.active_publisher(&plugin_id).await?;
        let Some(source_fingerprint) = publisher.prior_fingerprints.iter().rev().find(|prior| {
            reviewed_migration_id
                == migration_id(
                    &plugin_id,
                    prior,
                    &publisher.target_fingerprint,
                    profile.profile_id(),
                )
        }) else {
            return Err(Status::failed_precondition(
                "no reviewed migration matches this identity and profile",
            ));
        };
        let source_fingerprint = source_fingerprint.clone();
        let target_resolver =
            self.host_api()
                .profiles()
                .resolver(&plugin_id, &publisher.target_fingerprint, None)?;
        let target_owner = scope(&target_resolver, profile.epoch())?;

        let mut results = Vec::new();
        let mut removed_records = 0_u64;
        let mut reclaimed_bytes = 0_u64;
        let mut kept_protected_records = 0_u64;
        if let Some(storage) = self.private_storage() {
            match storage
                .revert_publisher_migration(&target_resolver, source_fingerprint.clone())
                .await
            {
                Ok(outcome) => {
                    let outcome = outcome.unwrap_or_default();
                    removed_records = removed_records.saturating_add(outcome.removed_records);
                    reclaimed_bytes = reclaimed_bytes.saturating_add(outcome.reclaimed_bytes);
                    kept_protected_records =
                        kept_protected_records.saturating_add(outcome.kept_protected_records);
                    results.push(rollback_result("private_persistence", outcome));
                }
                Err(error) => results.push(failed_rollback_result("private_persistence", error)),
            }
        } else {
            results.push(unavailable_rollback_result("private_persistence"));
        }

        let pool = self.host_api().shared_pool();
        match pool
            .revert_migrated_owner(
                target_owner,
                source_fingerprint.clone(),
                plugin_id.clone(),
                now,
            )
            .await
        {
            Ok(outcome) => {
                let outcome = outcome.unwrap_or_else(empty_revert);
                removed_records = removed_records.saturating_add(outcome.removed_records);
                reclaimed_bytes = reclaimed_bytes.saturating_add(outcome.reclaimed_bytes);
                kept_protected_records =
                    kept_protected_records.saturating_add(outcome.kept_protected_records);
                results.push(PublisherMigrationRollbackStoreResult {
                    store: "durable_shared".to_owned(),
                    status: "completed".to_owned(),
                    removed_records: outcome.removed_records,
                    reclaimed_bytes: outcome.reclaimed_bytes,
                    kept_protected_records: outcome.kept_protected_records,
                    error: None,
                });
            }
            Err(error) => results.push(PublisherMigrationRollbackStoreResult {
                store: "durable_shared".to_owned(),
                status: "failed".to_owned(),
                removed_records: 0,
                reclaimed_bytes: 0,
                kept_protected_records: 0,
                error: Some(error.message().to_owned()),
            }),
        }

        tracing::info!(
            migration_id = %reviewed_migration_id,
            plugin = %plugin_id,
            removed_records,
            reclaimed_bytes,
            "reviewed publisher migration rollback completed"
        );
        Ok(PublisherMigrationRollbackReport {
            migration_id: reviewed_migration_id,
            plugin_id,
            profile_id: profile.profile_id().to_owned(),
            source_publisher_fingerprint: source_fingerprint,
            target_publisher_fingerprint: publisher.target_fingerprint,
            results,
            removed_records,
            reclaimed_bytes,
            kept_protected_records,
        })
    }

    /// The old/new identity pair of one plugin in the active profile, from the pinned registry and
    /// the recorded trust history. A plugin RPC never supplies either value.
    async fn active_publisher(&self, plugin_id: &str) -> Result<ActivePublisher, Status> {
        let api = self.host_api();
        let provider_ids = api.third_party_provider_ids_for_plugin(plugin_id);
        let [provider_id] = provider_ids.as_slice() else {
            return Err(Status::failed_precondition(
                "third-party plugin has no unique registered provider",
            ));
        };
        let (registered_id, target_fingerprint) =
            api.third_party_identity(provider_id).ok_or_else(|| {
                Status::failed_precondition("third-party plugin identity is unavailable")
            })?;
        if registered_id != plugin_id {
            return Err(Status::failed_precondition(
                "registered plugin identity changed",
            ));
        }
        let declared_schemas = api.extension_schemas_for_provider(provider_id);
        let trust_dir = self.trust_dir();
        let trust_plugin = plugin_id.to_owned();
        let prior_fingerprints = tokio::task::spawn_blocking(move || {
            let trust = PublisherTrustStore::open(trust_dir).map_err(|error| error.to_string())?;
            Ok::<_, String>(trust.prior_fingerprints(&trust_plugin))
        })
        .await
        .map_err(|_| Status::internal("publisher trust worker failed"))?
        .map_err(|error| Status::unavailable(format!("publisher trust store: {error}")))?;
        let Some(source_fingerprint) = prior_fingerprints
            .iter()
            .rev()
            .find(|prior| *prior != &target_fingerprint)
            .cloned()
        else {
            return Err(Status::failed_precondition(
                "no previous publisher key is recorded for this plugin",
            ));
        };
        Ok(ActivePublisher {
            source_fingerprint,
            target_fingerprint,
            prior_fingerprints,
            declared_schemas,
        })
    }

    pub(crate) async fn publisher_migration_preview_at(
        &self,
        plugin_id: String,
        now: i64,
    ) -> Result<PublisherMigrationPreview, Status> {
        let profile = self.host_api().profiles().require()?;
        let profile_id = profile.profile_id().to_owned();
        let profile_epoch = profile.epoch();
        let publisher = self.active_publisher(&plugin_id).await?;
        let source_resolver =
            self.host_api()
                .profiles()
                .resolver(&plugin_id, &publisher.source_fingerprint, None)?;
        let target_resolver =
            self.host_api()
                .profiles()
                .resolver(&plugin_id, &publisher.target_fingerprint, None)?;
        let source_owner = scope(&source_resolver, profile_epoch)?;
        let target_owner = scope(&target_resolver, profile_epoch)?;
        let migration_id = migration_id(
            &plugin_id,
            &publisher.source_fingerprint,
            &publisher.target_fingerprint,
            &profile_id,
        );

        let pool = self.host_api().shared_pool();
        let source = pool.migration_source(source_owner, now).await?;
        let target_usage = pool.usage(target_owner, now).await?;
        let target_pairs = match pool.migration_source(target_owner, now).await? {
            Some(target_source) => target_source
                .records
                .into_iter()
                .filter_map(|record| {
                    record
                        .resource_id
                        .map(|resource| (record.dataset_id, resource))
                })
                .collect::<BTreeSet<_>>(),
            None => BTreeSet::new(),
        };

        let mut datasets: BTreeMap<String, PublisherMigrationDataset> = BTreeMap::new();
        let mut shared_records = 0_u64;
        let mut shared_eligible_records = 0_u64;
        let mut shared_eligible_bytes = 0_u64;
        let mut shared_conflict_records = 0_u64;
        let mut shared_incompatible_schema_records = 0_u64;
        let mut shared_archived_records = 0_u64;
        if let Some(source) = source {
            shared_archived_records = source.archived_records;
            for record in source.records {
                let entry = datasets
                    .entry(record.dataset_id.clone())
                    .or_insert_with(|| PublisherMigrationDataset {
                        dataset_id: record.dataset_id.clone(),
                        records: 0,
                        eligible_records: 0,
                        eligible_bytes: 0,
                        conflict_records: 0,
                        incompatible_schema_records: 0,
                        incompatible_schema_ids: Vec::new(),
                        logical_bytes: 0,
                    });
                entry.records += 1;
                entry.logical_bytes = entry.logical_bytes.saturating_add(record.size_bytes);
                shared_records += 1;
                if !record_compatible(&plugin_id, &record, &publisher.declared_schemas) {
                    entry.incompatible_schema_records += 1;
                    shared_incompatible_schema_records += 1;
                    if entry.incompatible_schema_ids.len() < MAX_REPORTED_SCHEMAS
                        && !entry.incompatible_schema_ids.contains(&record.schema_id)
                    {
                        entry.incompatible_schema_ids.push(record.schema_id);
                    }
                    continue;
                }
                let conflict = record.resource_id.as_ref().is_some_and(|resource| {
                    target_pairs.contains(&(record.dataset_id.clone(), resource.clone()))
                });
                if conflict {
                    entry.conflict_records += 1;
                    shared_conflict_records += 1;
                    continue;
                }
                entry.eligible_records += 1;
                entry.eligible_bytes = entry.eligible_bytes.saturating_add(record.size_bytes);
                shared_eligible_records += 1;
                shared_eligible_bytes = shared_eligible_bytes.saturating_add(record.size_bytes);
            }
        }

        let private = match self.private_storage() {
            Some(storage) => Some(
                storage
                    .publisher_migration_preview(&source_resolver, &target_resolver)
                    .await?,
            ),
            None => None,
        };
        let private_store_available = private.is_some();
        let private = private.flatten().unwrap_or_default();

        let can_migrate = shared_eligible_records > 0 || private.eligible_records > 0;
        let token = gamenode_platform::random::nonce();
        self.migration_reviews().issue(ReviewedMigration {
            token: token.clone(),
            migration_id: migration_id.clone(),
            plugin_id: plugin_id.clone(),
            profile_id: profile_id.clone(),
            profile_epoch,
            source_fingerprint: publisher.source_fingerprint.clone(),
            target_fingerprint: publisher.target_fingerprint.clone(),
            source_owner_key: *source_resolver.storage_owner().key(),
            target_owner_key: *target_resolver.storage_owner().key(),
        });
        Ok(PublisherMigrationPreview {
            token,
            migration_id,
            generated_at: now,
            plugin_id,
            profile_id,
            profile_epoch,
            source_publisher_fingerprint: publisher.source_fingerprint,
            target_publisher_fingerprint: publisher.target_fingerprint,
            historical_publisher_fingerprints: publisher.prior_fingerprints,
            datasets: datasets.into_values().collect(),
            shared_records,
            shared_eligible_records,
            shared_eligible_bytes,
            shared_conflict_records,
            shared_incompatible_schema_records,
            shared_archived_records,
            shared_target_records: target_usage.as_ref().map_or(0, |usage| usage.records),
            shared_target_logical_bytes: target_usage
                .as_ref()
                .map_or(0, |usage| usage.logical_bytes),
            private_store_available,
            private_records: private.records,
            private_eligible_records: private.eligible_records,
            private_eligible_bytes: private.eligible_bytes,
            private_conflict_records: private.conflict_records,
            private_expired_records: private.expired_records,
            private_archived_records: private.archived_records,
            private_protected_records: private.protected_records,
            private_target_records: private.target_records,
            private_target_logical_bytes: private.target_logical_bytes,
            secrets_excluded: true,
            grants_excluded: true,
            sharing_review_required: true,
            retention_review_required: true,
            can_migrate,
        })
    }

    pub(crate) async fn publisher_migration_apply_at(
        &self,
        token: String,
        expected_profile_id: String,
        confirmed: bool,
        now: i64,
    ) -> Result<PublisherMigrationReport, Status> {
        if !confirmed {
            return Err(Status::failed_precondition(
                "a reviewed data migration requires confirmation",
            ));
        }
        let review = self
            .migration_reviews()
            .get(&token)
            .ok_or_else(|| Status::failed_precondition("migration review expired or was used"))?;
        let profile = self.host_api().profiles().require()?;
        if profile.profile_id() != expected_profile_id
            || profile.profile_id() != review.profile_id
            || profile.epoch() != review.profile_epoch
        {
            return Err(Status::failed_precondition(
                "the account profile changed; create a new migration review",
            ));
        }
        let publisher = self.active_publisher(&review.plugin_id).await?;
        if publisher.target_fingerprint != review.target_fingerprint
            || publisher.source_fingerprint != review.source_fingerprint
        {
            return Err(Status::failed_precondition(
                "plugin identity changed since the migration review",
            ));
        }
        if migration_id(
            &review.plugin_id,
            &review.source_fingerprint,
            &review.target_fingerprint,
            profile.profile_id(),
        ) != review.migration_id
        {
            return Err(Status::failed_precondition(
                "migration review does not match the active identity",
            ));
        }
        let source_resolver = self.host_api().profiles().resolver(
            &review.plugin_id,
            &review.source_fingerprint,
            None,
        )?;
        let target_resolver = self.host_api().profiles().resolver(
            &review.plugin_id,
            &review.target_fingerprint,
            None,
        )?;
        let source_owner = scope(&source_resolver, profile.epoch())?;
        let target_owner = scope(&target_resolver, profile.epoch())?;
        if *source_resolver.storage_owner().key() != review.source_owner_key
            || *target_resolver.storage_owner().key() != review.target_owner_key
        {
            return Err(Status::failed_precondition(
                "plugin identity changed since the migration review",
            ));
        }

        let mut results = Vec::new();
        let mut copied_records = 0_u64;
        let mut copied_bytes = 0_u64;
        let mut conflict_records = 0_u64;
        let mut skipped_over_quota_records = 0_u64;
        let mut incompatible_schema_records = 0_u64;

        if let Some(storage) = self.private_storage() {
            match storage
                .migrate_publisher_data(&source_resolver, &target_resolver)
                .await
            {
                Ok(outcome) => {
                    let outcome = private_completed(&mut results, outcome);
                    copied_records = copied_records.saturating_add(outcome.copied_records);
                    copied_bytes = copied_bytes.saturating_add(outcome.copied_bytes);
                    conflict_records = conflict_records.saturating_add(outcome.conflict_records);
                }
                Err(error) => {
                    results.push(failed_store_result("private_persistence", error.message()))
                }
            }
        } else {
            results.push(PublisherMigrationStoreResult {
                store: "private_persistence".to_owned(),
                status: "unavailable".to_owned(),
                copied_records: 0,
                copied_bytes: 0,
                conflict_records: 0,
                skipped_over_quota_records: 0,
                expired_records: 0,
                archived_records: 0,
                error: Some("private plugin data is unavailable on this host".to_owned()),
            });
        }

        // The compatible set is recomputed against the live source so a record that appeared or
        // changed since the preview is not copied blindly.
        let pool = self.host_api().shared_pool();
        let source = pool.migration_source(source_owner, now).await?;
        match source {
            None => results.push(shared_store_result(&empty_migration_outcome())),
            Some(source) => {
                let mut compatible_ids = Vec::new();
                for record in &source.records {
                    if record_compatible(&review.plugin_id, record, &publisher.declared_schemas) {
                        compatible_ids.push(record.record_id.clone());
                    } else {
                        incompatible_schema_records += 1;
                    }
                }
                match pool
                    .migrate_owner(source_owner, target_owner, compatible_ids, now)
                    .await
                {
                    Ok(outcome) => {
                        let outcome = outcome.unwrap_or_else(empty_migration_outcome);
                        let result = shared_store_result(&outcome);
                        copied_records = copied_records.saturating_add(result.copied_records);
                        copied_bytes = copied_bytes.saturating_add(result.copied_bytes);
                        conflict_records = conflict_records.saturating_add(result.conflict_records);
                        skipped_over_quota_records = skipped_over_quota_records
                            .saturating_add(result.skipped_over_quota_records);
                        results.push(result);
                    }
                    Err(error) => {
                        results.push(failed_store_result("durable_shared", error.message()))
                    }
                }
            }
        }

        // Single use: even a partial failure consumes the reviewed snapshot and needs a new
        // preview for a retry.
        self.migration_reviews().take(&token);
        tracing::info!(
            %token,
            plugin = %review.plugin_id,
            source = %review.source_fingerprint,
            target = %review.target_fingerprint,
            copied_records,
            copied_bytes,
            "reviewed publisher data migration completed"
        );
        Ok(PublisherMigrationReport {
            token,
            migration_id: review.migration_id,
            plugin_id: review.plugin_id,
            profile_id: review.profile_id,
            source_publisher_fingerprint: review.source_fingerprint,
            target_publisher_fingerprint: review.target_fingerprint,
            results,
            copied_records,
            copied_bytes,
            conflict_records,
            skipped_over_quota_records,
            incompatible_schema_records,
            secrets_excluded: true,
            grants_excluded: true,
            sharing_review_required: true,
            retention_review_required: true,
        })
    }
}

fn scope(resolver: &ExternalNamespaceResolver, profile_epoch: u64) -> Result<OwnerScope, Status> {
    OwnerScope::new(*resolver.storage_owner().key(), profile_epoch)
        .map_err(|_| Status::failed_precondition("plugin profile is invalid"))
}

fn private_completed(
    results: &mut Vec<PublisherMigrationStoreResult>,
    outcome: PrivateMigrationOutcome,
) -> PrivateMigrationOutcome {
    results.push(PublisherMigrationStoreResult {
        store: "private_persistence".to_owned(),
        status: "completed".to_owned(),
        copied_records: outcome.copied_records,
        copied_bytes: outcome.copied_bytes,
        conflict_records: outcome.conflict_records,
        skipped_over_quota_records: 0,
        expired_records: outcome.expired_records,
        archived_records: outcome.archived_records,
        error: None,
    });
    outcome
}

fn shared_store_result(outcome: &MigrationOutcome) -> PublisherMigrationStoreResult {
    let conflicts = outcome
        .skipped
        .iter()
        .filter(|skip| skip.reason == "conflict")
        .count() as u64;
    PublisherMigrationStoreResult {
        store: "durable_shared".to_owned(),
        status: "completed".to_owned(),
        copied_records: outcome.copied.len() as u64,
        copied_bytes: outcome.copied.iter().map(|record| record.size_bytes).sum(),
        conflict_records: conflicts,
        skipped_over_quota_records: outcome
            .skipped
            .iter()
            .filter(|skip| skip.reason == "over_quota")
            .count() as u64,
        expired_records: 0,
        archived_records: 0,
        error: None,
    }
}

fn failed_store_result(store: &str, error: &str) -> PublisherMigrationStoreResult {
    PublisherMigrationStoreResult {
        store: store.to_owned(),
        status: "failed".to_owned(),
        copied_records: 0,
        copied_bytes: 0,
        conflict_records: 0,
        skipped_over_quota_records: 0,
        expired_records: 0,
        archived_records: 0,
        error: Some(error.to_owned()),
    }
}

fn rollback_result(
    store: &str,
    outcome: PrivateMigrationRevert,
) -> PublisherMigrationRollbackStoreResult {
    PublisherMigrationRollbackStoreResult {
        store: store.to_owned(),
        status: "completed".to_owned(),
        removed_records: outcome.removed_records,
        reclaimed_bytes: outcome.reclaimed_bytes,
        kept_protected_records: outcome.kept_protected_records,
        error: None,
    }
}

fn failed_rollback_result(store: &str, error: Status) -> PublisherMigrationRollbackStoreResult {
    PublisherMigrationRollbackStoreResult {
        store: store.to_owned(),
        status: "failed".to_owned(),
        removed_records: 0,
        reclaimed_bytes: 0,
        kept_protected_records: 0,
        error: Some(error.message().to_owned()),
    }
}

fn unavailable_rollback_result(store: &str) -> PublisherMigrationRollbackStoreResult {
    PublisherMigrationRollbackStoreResult {
        store: store.to_owned(),
        status: "unavailable".to_owned(),
        removed_records: 0,
        reclaimed_bytes: 0,
        kept_protected_records: 0,
        error: Some("private plugin data is unavailable on this host".to_owned()),
    }
}

fn empty_revert() -> MigrationRevertOutcome {
    MigrationRevertOutcome {
        removed_records: 0,
        reclaimed_bytes: 0,
        kept_protected_records: 0,
        pool_revision: 0,
    }
}

fn unix_now() -> Result<i64, Status> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs().max(1) as i64)
        .map_err(|_| Status::failed_precondition("the system clock is before the Unix epoch"))
}

/// Previews the compatible data a re-trusted publisher identity could inherit. Host administration
/// only: this is a Tauri command, never a plugin RPC.
#[tauri::command]
pub async fn plugin_publisher_migration_preview(
    lifecycle: tauri::State<'_, Arc<ThirdPartyPluginLifecycle>>,
    plugin_id: String,
) -> Result<PublisherMigrationPreview, String> {
    lifecycle
        .publisher_migration_preview(plugin_id)
        .await
        .map_err(|error| error.message().to_owned())
}

/// Applies one confirmed, single-use migration preview. An unconfirmed, cancelled or stale call
/// changes nothing.
#[tauri::command]
pub async fn plugin_publisher_migration_apply(
    lifecycle: tauri::State<'_, Arc<ThirdPartyPluginLifecycle>>,
    token: String,
    expected_profile_id: String,
    confirmed: bool,
) -> Result<PublisherMigrationReport, String> {
    lifecycle
        .publisher_migration_apply(token, expected_profile_id, confirmed)
        .await
        .map_err(|error| error.message().to_owned())
}

/// Discards one migration preview without changing data.
#[tauri::command]
pub async fn plugin_publisher_migration_cancel(
    lifecycle: tauri::State<'_, Arc<ThirdPartyPluginLifecycle>>,
    token: String,
) -> Result<(), String> {
    lifecycle
        .publisher_migration_cancel(token)
        .await
        .map_err(|error| error.message().to_owned())
}

/// Removes one migration's copies from the new publisher identity after an explicit confirmation.
/// The old identity keeps its data.
#[tauri::command]
pub async fn plugin_publisher_migration_rollback(
    lifecycle: tauri::State<'_, Arc<ThirdPartyPluginLifecycle>>,
    plugin_id: String,
    migration_id: String,
    expected_profile_id: String,
    confirmed: bool,
) -> Result<PublisherMigrationRollbackReport, String> {
    lifecycle
        .publisher_migration_rollback(plugin_id, migration_id, expected_profile_id, confirmed)
        .await
        .map_err(|error| error.message().to_owned())
}

#[cfg(test)]
mod tests;
