//! Owner-scoped administration for a plugin's durable private key/value namespace.
//!
//! These routes expose logical metadata only. They never return values, store paths, or a
//! namespace selector supplied by the plugin. Ownership and cursor bindings come from the
//! authenticated session.

use gamenode_platform::plugin_storage::{MAX_TTL_SECONDS, QUOTA_BYTES, RetentionPolicy};
use gamenode_plugin_protocol::{capability as cap, proto::*};
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::mutations::{mutation_key, reply, request_hash};
use super::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, ResourceApi};
use crate::plugin_host::library::CommitGuard;
use crate::plugin_host::provider::ResourceCaller;

const MAX_DELETE_ENTRIES: usize = 256;
const MAX_ENTRY_ID_BYTES: usize = 64;

fn category(value: &str) -> Result<&'static str, Status> {
    if value == "private" {
        Ok("private")
    } else {
        Err(Status::invalid_argument("category must be private"))
    }
}

fn owner_binding(caller: &ResourceCaller, revision: u64) -> Result<String, Status> {
    let resolver = caller
        .namespace
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("account namespace required"))?;
    let storage_owner = resolver.storage_owner();
    let owner = storage_owner.key();
    let owner_hex: String = owner.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "persistence:{owner_hex}:{}:{revision}:private",
        caller.profile_epoch
    ))
}

fn entry_id(caller: &ResourceCaller, key: &str) -> Result<String, Status> {
    let resolver = caller
        .namespace
        .as_ref()
        .ok_or_else(|| Status::failed_precondition("account namespace required"))?;
    let mut digest = Sha256::new();
    digest.update(b"gamenode.persistence.entry.v1\0");
    digest.update(resolver.storage_owner().key());
    digest.update((key.len() as u64).to_be_bytes());
    digest.update(key.as_bytes());
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn validate_ids(ids: &[String]) -> Result<(), Status> {
    if ids.len() > MAX_DELETE_ENTRIES {
        return Err(Status::invalid_argument(format!(
            "at most {MAX_DELETE_ENTRIES} entries may be deleted"
        )));
    }
    let mut unique = std::collections::HashSet::new();
    for id in ids {
        if id.len() != MAX_ENTRY_ID_BYTES
            || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !unique.insert(id)
        {
            return Err(Status::invalid_argument(
                "entry ids must be unique opaque ids",
            ));
        }
    }
    Ok(())
}

impl ResourceApi {
    pub(super) async fn persistence_usage(
        &self,
        request: Request<PersistenceUsageRequest>,
    ) -> Result<Response<PersistenceUsageReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::STORAGE_PERSIST)?;
        let usage = self
            .in_namespace(&caller, |namespace| namespace.store_usage())
            .await?;
        self.providers
            .authorize_resource(request.context.as_ref(), cap::STORAGE_PERSIST)?;
        Ok(Response::new(PersistenceUsageReply {
            logical_bytes: usage.logical_bytes,
            physical_bytes: usage.physical_bytes_estimate,
            quota_bytes: QUOTA_BYTES,
            categories: usage
                .categories
                .into_iter()
                .map(|category| PersistenceCategoryUsage {
                    category: category.category,
                    records: category.records,
                    logical_bytes: category.logical_bytes,
                    protected_records: category.protected_records,
                })
                .collect(),
            // SQLite metadata and free pages belong to the whole namespace database.
            physical_bytes_namespace_wide: true,
        }))
    }

    pub(super) async fn persistence_list(
        &self,
        request: Request<PersistenceListRequest>,
    ) -> Result<Response<PersistenceListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        let category = category(&request.category)?;
        let caller = self
            .providers
            .authorize_resource(page.context.as_ref(), cap::STORAGE_PERSIST)?;
        let limit = match page.page_size as usize {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };
        let (revision, entries) = self
            .in_namespace(&caller, |namespace| {
                Ok((namespace.revision()?, namespace.persistence_entries()?))
            })
            .await?;
        let binding = owner_binding(&caller, revision)?;
        let after = if page.cursor.is_empty() {
            None
        } else {
            let key = self
                .files
                .open_cursor(&caller.session_id, &binding, &page.cursor)
                .ok_or_else(|| {
                    Status::failed_precondition("persistence cursor is invalid or stale")
                })?;
            Some(key)
        };
        let mut records = entries
            .into_iter()
            .filter(|entry| after.as_ref().is_none_or(|key| entry.key > *key));
        let selected: Vec<_> = records.by_ref().take(limit).collect();
        let has_more = records.next().is_some();
        let next_cursor = if has_more {
            selected
                .last()
                .map(|last| {
                    self.files
                        .seal_cursor(&caller.session_id, &binding, &last.key)
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        self.providers
            .authorize_resource(page.context.as_ref(), cap::STORAGE_PERSIST)?;
        let entries = selected
            .into_iter()
            .map(|entry| {
                Ok(PersistenceEntry {
                    entry_id: entry_id(&caller, &entry.key)?,
                    category: category.to_owned(),
                    logical_key: entry.key,
                    size_bytes: entry.size_bytes,
                    last_used_at: entry.last_used_at,
                    user_protected: entry.user_protected,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;
        Ok(Response::new(PersistenceListReply {
            entries,
            next_cursor,
        }))
    }

    pub(super) async fn persistence_delete(
        &self,
        request: Request<PersistenceDeleteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_PERSIST)?;
        validate_ids(&request.entry_ids)?;
        let idempotency = mutation_key(mutation)?.to_owned();
        let ids = request.entry_ids;
        let mut hash_parts = vec![
            "persistence.delete".to_owned(),
            mutation.expected_revision.to_string(),
        ];
        hash_parts.extend(ids.iter().cloned());
        let hash_refs: Vec<_> = hash_parts.iter().map(String::as_str).collect();
        let hash = request_hash(&hash_refs);
        let guard = self.guard(context, cap::STORAGE_PERSIST, caller.grant_revision);
        self.persist_delete(
            caller,
            ids,
            idempotency,
            mutation.expected_revision,
            hash,
            guard,
        )
        .await
        .map(Response::new)
    }

    async fn persist_delete(
        &self,
        caller: ResourceCaller,
        ids: Vec<String>,
        idempotency: String,
        expected: u64,
        hash: [u8; 32],
        guard: CommitGuard,
    ) -> Result<MutationReply, Status> {
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        self.in_namespace(&caller, move |namespace| {
            if let Some(original) = mutations.replay(&remember_as, &idempotency, hash)? {
                return Ok(original);
            }
            let current = namespace.revision()?;
            if current != expected {
                return Ok(reply("private", current, MutationOutcome::RevisionConflict));
            }
            let entries = namespace.persistence_entries()?;
            let mut keys = Vec::with_capacity(ids.len());
            for requested in &ids {
                let found = entries
                    .iter()
                    .find(|entry| {
                        entry_id(&remember_as, &entry.key).is_ok_and(|id| id == *requested)
                    })
                    .ok_or_else(|| Status::not_found("persistence entry not found"))?;
                keys.push(found.key.clone());
            }
            let (reclaimed, revision) = namespace.delete_persistence_entries(&keys, &guard)?;
            let answer = reply(
                "private",
                revision,
                if reclaimed > 0 {
                    MutationOutcome::Applied
                } else {
                    MutationOutcome::AlreadyApplied
                },
            );
            mutations.remember(&remember_as, &idempotency, hash, &answer);
            Ok(answer)
        })
        .await
    }

    pub(super) async fn persistence_clear_entries(
        &self,
        request: Request<PersistenceClearRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        category(&request.category)?;
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_PERSIST)?;
        let idempotency = mutation_key(mutation)?.to_owned();
        let hash = request_hash(&[
            "persistence.clear",
            "private",
            &mutation.expected_revision.to_string(),
        ]);
        let guard = self.guard(context, cap::STORAGE_PERSIST, caller.grant_revision);
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        let expected = mutation.expected_revision;
        let answer = self
            .in_namespace(&caller, move |namespace| {
                if let Some(original) = mutations.replay(&remember_as, &idempotency, hash)? {
                    return Ok(original);
                }
                let current = namespace.revision()?;
                if current != expected {
                    return Ok(reply("private", current, MutationOutcome::RevisionConflict));
                }
                let entries = namespace.persistence_entries()?;
                // A plugin clear never removes user-protected records; an explicit delete of a
                // protected entry fails below instead.
                let keys: Vec<_> = entries
                    .into_iter()
                    .filter(|entry| !entry.user_protected)
                    .map(|entry| entry.key)
                    .collect();
                let (reclaimed, revision) = namespace.delete_persistence_entries(&keys, &guard)?;
                let answer = reply(
                    "private",
                    revision,
                    if reclaimed > 0 {
                        MutationOutcome::Applied
                    } else {
                        MutationOutcome::AlreadyApplied
                    },
                );
                mutations.remember(&remember_as, &idempotency, hash, &answer);
                Ok(answer)
            })
            .await?;
        Ok(Response::new(answer))
    }

    pub(super) async fn persistence_set_policy(
        &self,
        request: Request<RetentionPolicyRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        category(&request.category)?;
        if request.max_age_seconds > MAX_TTL_SECONDS || request.max_bytes > QUOTA_BYTES {
            return Err(Status::invalid_argument(
                "retention limits exceed the storage maximum",
            ));
        }
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation
            .context
            .clone()
            .ok_or_else(|| Status::invalid_argument("context is required"))?;
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_PERSIST)?;
        let idempotency = mutation_key(mutation)?.to_owned();
        let hash = request_hash(&[
            "persistence.retention",
            "private",
            &mutation.expected_revision.to_string(),
            &request.max_age_seconds.to_string(),
            &request.max_bytes.to_string(),
        ]);
        let guard = self.guard(context, cap::STORAGE_PERSIST, caller.grant_revision);
        let policy = RetentionPolicy {
            max_age_seconds: request.max_age_seconds,
            max_bytes: request.max_bytes,
        };
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        let expected = mutation.expected_revision;
        let answer = self
            .in_namespace(&caller, move |namespace| {
                if let Some(original) = mutations.replay(&remember_as, &idempotency, hash)? {
                    return Ok(original);
                }
                let current = namespace.revision()?;
                if current != expected {
                    return Ok(reply("private", current, MutationOutcome::RevisionConflict));
                }
                let (reclaimed, revision) =
                    namespace.set_retention_policy("private", policy, &guard)?;
                let _ = reclaimed;
                let answer = reply("private", revision, MutationOutcome::Applied);
                mutations.remember(&remember_as, &idempotency, hash, &answer);
                Ok(answer)
            })
            .await?;
        Ok(Response::new(answer))
    }
}
