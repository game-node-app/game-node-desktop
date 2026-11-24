//! Private key/value storage of `ResourceHost`: `StorageGet`, `StoragePut`, `StorageDelete`.
//!
//! The caller's namespace is the host-authenticated plugin/publisher/profile identity
//! ([`crate::plugin_host::storage`]); a request names only a key. Reading needs
//! `storage.read`, writing `storage.write`, deleting `storage.delete`, and a write that asks for
//! `PERSISTENT` durability also needs `storage.persist`: without it the call is denied, never
//! quietly kept in memory. Writes and deletes carry an idempotency key and the key's expected
//! revision (0 for a key that does not exist), and commit under a recheck of the caller's
//! permissions.

use gamenode_plugin_protocol::{capability as cap, proto::*};
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::ResourceApi;
use super::mutations::{mutation_key, reply, request_hash};
use crate::plugin_host::provider::ResourceCaller;
use crate::plugin_host::storage::{
    DeleteOutcome, Durability, MAX_KEY_BYTES, MAX_TTL_SECONDS, MAX_VALUE_BYTES, Namespace,
    PutOutcome, QUOTA_BYTES,
};

fn storage_key(key: &str) -> Result<String, Status> {
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(Status::invalid_argument(format!(
            "key is required: 1 to {MAX_KEY_BYTES} bytes without control characters"
        )));
    }
    Ok(key.to_owned())
}

fn proto_durability(durability: Durability) -> StorageDurability {
    match durability {
        Durability::SessionOnly => StorageDurability::SessionOnly,
        Durability::Persistent => StorageDurability::Persistent,
    }
}

impl ResourceApi {
    /// Runs `work` on the caller's namespace on a blocking worker.
    pub(super) async fn in_namespace<T: Send + 'static>(
        &self,
        caller: &ResourceCaller,
        work: impl FnOnce(&mut Namespace<'_>) -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let providers = self.providers.clone();
        self.storage
            .run(caller, move |id| providers.session_is_open(id), work)
            .await
    }

    /// Runs host-initiated user actions in the same private namespace without pretending they
    /// came from a live plugin session. The caller must be built from the current profile's
    /// authenticated identity and grants.
    pub(super) async fn in_host_namespace<T: Send + 'static>(
        &self,
        caller: &ResourceCaller,
        work: impl FnOnce(&mut Namespace<'_>) -> Result<T, Status> + Send + 'static,
    ) -> Result<T, Status> {
        let resolver = caller.namespace.clone().ok_or_else(|| {
            Status::failed_precondition("official providers use trusted host adapters")
        })?;
        self.storage
            .run_for_host(&resolver, caller.profile_epoch, work)
            .await
    }

    pub(super) async fn get_stored(
        &self,
        request: Request<StorageGetRequest>,
    ) -> Result<Response<StorageGetReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::STORAGE_READ)?;
        let key = storage_key(&request.key)?;
        let found = self
            .in_namespace(&caller, move |namespace| namespace.get(&key))
            .await?;
        // The value was read for a call that may since have lost its permission.
        self.providers
            .authorize_resource(request.context.as_ref(), cap::STORAGE_READ)?;
        Ok(Response::new(match found {
            None => StorageGetReply {
                found: false,
                ..StorageGetReply::default()
            },
            Some(found) => StorageGetReply {
                value: found.value,
                revision: found.revision,
                found: true,
                durability: proto_durability(found.durability) as i32,
                expires_at: found.expires_at.unwrap_or(0),
            },
        }))
    }

    pub(super) async fn put_stored(
        &self,
        request: Request<StoragePutRequest>,
    ) -> Result<Response<StorageMutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_WRITE)?;
        let idempotency = mutation_key(mutation)?.to_owned();
        let key = storage_key(&request.key)?;
        let durability = match StorageDurability::try_from(request.durability) {
            Ok(StorageDurability::SessionOnly) => Durability::SessionOnly,
            Ok(StorageDurability::Persistent) => Durability::Persistent,
            _ => {
                return Err(Status::invalid_argument(
                    "durability is required: SESSION_ONLY or PERSISTENT",
                ));
            }
        };
        if durability == Durability::Persistent {
            // A missing storage.persist is PERMISSION_DENIED, never a silent downgrade.
            self.providers
                .authorize_resource(Some(&context), cap::STORAGE_PERSIST)?;
        }
        if request.value.len() > MAX_VALUE_BYTES {
            return Err(Status::invalid_argument(format!(
                "value is larger than {MAX_VALUE_BYTES} bytes"
            )));
        }
        if request.ttl_seconds > MAX_TTL_SECONDS {
            return Err(Status::invalid_argument(format!(
                "ttl_seconds is longer than {MAX_TTL_SECONDS} (30 days)"
            )));
        }
        let hash = request_hash(&[
            "storage.put",
            &key,
            &mutation.expected_revision.to_string(),
            if durability == Durability::Persistent {
                "persistent"
            } else {
                "session"
            },
            &request.ttl_seconds.to_string(),
            &format!("{:x}", Sha256::digest(&request.value)),
        ]);
        let write_guard = self.guard(context.clone(), cap::STORAGE_WRITE, caller.grant_revision);
        let persist_guard = self.guard(context, cap::STORAGE_PERSIST, caller.grant_revision);
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        let expected = mutation.expected_revision;
        let ttl = request.ttl_seconds;
        let value = request.value;
        let answered = self
            .in_namespace(&caller, move |namespace| {
                let id = key.clone();
                if let Some((original, expires_at)) =
                    mutations.replay_with(&remember_as, &idempotency, hash)?
                {
                    let usage = namespace.usage(durability)?;
                    return Ok(StorageMutationReply {
                        revision: original.revision,
                        usage_bytes: usage,
                        quota_bytes: QUOTA_BYTES,
                        outcome: original.outcome,
                        durability: proto_durability(durability) as i32,
                        expires_at,
                    });
                }
                let guard: crate::plugin_host::library::CommitGuard = Box::new(move || {
                    write_guard()?;
                    if durability == Durability::Persistent {
                        persist_guard()?;
                    }
                    Ok(())
                });
                match namespace.put(&key, value, durability, ttl, expected, &guard)? {
                    PutOutcome::Applied {
                        revision,
                        expires_at,
                        usage,
                    } => {
                        let answer = reply(&id, revision, MutationOutcome::Applied);
                        let expires = expires_at.unwrap_or(0);
                        mutations.remember_with(&remember_as, &idempotency, hash, &answer, expires);
                        Ok(StorageMutationReply {
                            revision,
                            usage_bytes: usage,
                            quota_bytes: QUOTA_BYTES,
                            outcome: MutationOutcome::Applied as i32,
                            durability: proto_durability(durability) as i32,
                            expires_at: expires,
                        })
                    }
                    PutOutcome::Conflict { current } => Ok(StorageMutationReply {
                        revision: current,
                        usage_bytes: namespace.usage(durability)?,
                        quota_bytes: QUOTA_BYTES,
                        outcome: MutationOutcome::RevisionConflict as i32,
                        durability: proto_durability(durability) as i32,
                        expires_at: 0,
                    }),
                }
            })
            .await?;
        Ok(Response::new(answered))
    }

    pub(super) async fn delete_stored(
        &self,
        request: Request<StorageDeleteRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let context = mutation.context.clone().unwrap_or_default();
        let caller = self
            .providers
            .authorize_resource(Some(&context), cap::STORAGE_DELETE)?;
        let idempotency = mutation_key(mutation)?.to_owned();
        let key = storage_key(&request.key)?;
        let hash = request_hash(&[
            "storage.delete",
            &key,
            &mutation.expected_revision.to_string(),
        ]);
        let guard = self.guard(context, cap::STORAGE_DELETE, caller.grant_revision);
        let mutations = self.mutations.clone();
        let remember_as = caller.clone();
        let expected = mutation.expected_revision;
        let answered = self
            .in_namespace(&caller, move |namespace| {
                if let Some((original, _)) =
                    mutations.replay_with(&remember_as, &idempotency, hash)?
                {
                    return Ok(original);
                }
                let (outcome, revision, commit) = match namespace.delete(&key, expected, &guard)? {
                    DeleteOutcome::Deleted { revision } => {
                        (MutationOutcome::Applied, revision, true)
                    }
                    DeleteOutcome::Absent => (MutationOutcome::AlreadyApplied, 0, true),
                    DeleteOutcome::Conflict { current } => {
                        (MutationOutcome::RevisionConflict, current, false)
                    }
                };
                let answer = reply(&key, revision, outcome);
                if commit {
                    mutations.remember_with(&remember_as, &idempotency, hash, &answer, 0);
                }
                Ok(answer)
            })
            .await?;
        Ok(Response::new(answered))
    }
}
