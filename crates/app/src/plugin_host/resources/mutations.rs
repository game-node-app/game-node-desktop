//! Display-metadata and favorite services of `ResourceHost`.
//!
//! Both are mutations of the caller's own library entries: the plugin names an installation (or
//! a GameNode game one of its own entries is matched to), the host validates the request against
//! Core rules, and the engine commits it. Every mutation needs an idempotency key. A retried key
//! with the same request returns the original committed outcome; the same key with another
//! request is refused. The table is in memory (bounded, 24 hours), so a replay after a restart
//! falls back to the resource's own guard: metadata is protected by its revision, and favorites
//! are desired-state writes that are idempotent by nature.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gamenode_core::library::DisplayWrite;
use gamenode_plugin_protocol::{capability as cap, proto::*};
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE, ResourceApi, Scope, no_common_data, stale_cursor};
use crate::plugin_host::library::{CommitGuard, CursorBinding, CursorError, FavoriteWrite};
use crate::plugin_host::provider::ResourceCaller;

/// The only display field a plugin may write.
pub const DISPLAY_NAME: &str = "display_name";
const MAX_DISPLAY_NAME_BYTES: usize = 256;
const MAX_KEY_BYTES: usize = 128;
const IDEMPOTENCY_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const IDEMPOTENCY_PER_PLUGIN: usize = 1024;

struct Seen {
    hash: [u8; 32],
    reply: MutationReply,
    /// A fact some replies carry beyond [`MutationReply`] (a storage value's expiry); 0 if none.
    extra: i64,
    at: Instant,
}

/// Serializes mutating calls and remembers their committed outcomes by
/// `(authenticated namespace, profile epoch, idempotency key)`; a new profile starts with no keys.
#[derive(Default)]
pub(super) struct Mutations {
    pub(super) lock: tokio::sync::Mutex<()>,
    seen: Mutex<HashMap<(String, u64, String), Seen>>,
}

impl Mutations {
    pub(super) fn replay(
        &self,
        caller: &ResourceCaller,
        key: &str,
        hash: [u8; 32],
    ) -> Result<Option<MutationReply>, Status> {
        Ok(self.replay_with(caller, key, hash)?.map(|(reply, _)| reply))
    }

    /// [`replay`](Self::replay), also returning the `extra` value stored with the outcome.
    pub(crate) fn replay_with(
        &self,
        caller: &ResourceCaller,
        key: &str,
        hash: [u8; 32],
    ) -> Result<Option<(MutationReply, i64)>, Status> {
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        let id = (mutation_owner(caller), caller.profile_epoch, key.to_owned());
        match seen.get(&id) {
            Some(entry) if entry.at.elapsed() >= IDEMPOTENCY_TTL => {
                seen.remove(&id);
                Ok(None)
            }
            Some(entry) if entry.hash == hash => Ok(Some((entry.reply.clone(), entry.extra))),
            Some(_) => Err(Status::failed_precondition(
                "the idempotency key was already used for a different request",
            )),
            None => Ok(None),
        }
    }

    pub(super) fn remember(
        &self,
        caller: &ResourceCaller,
        key: &str,
        hash: [u8; 32],
        reply: &MutationReply,
    ) {
        self.remember_with(caller, key, hash, reply, 0);
    }

    /// [`remember`](Self::remember), keeping `extra` beside the outcome.
    pub(crate) fn remember_with(
        &self,
        caller: &ResourceCaller,
        key: &str,
        hash: [u8; 32],
        reply: &MutationReply,
        extra: i64,
    ) {
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        seen.retain(|_, entry| entry.at.elapsed() < IDEMPOTENCY_TTL);
        let own = |(provider, epoch, _): &(String, u64, String)| {
            *provider == mutation_owner(caller) && *epoch == caller.profile_epoch
        };
        if seen.keys().filter(|id| own(id)).count() >= IDEMPOTENCY_PER_PLUGIN
            && let Some(oldest) = seen
                .iter()
                .filter(|(id, _)| own(id))
                .min_by_key(|(_, entry)| entry.at)
                .map(|(id, _)| id.clone())
        {
            seen.remove(&oldest);
        }
        seen.insert(
            (mutation_owner(caller), caller.profile_epoch, key.to_owned()),
            Seen {
                hash,
                reply: reply.clone(),
                extra,
                at: Instant::now(),
            },
        );
    }
}

// Provider-free extensions share an empty provider id; their authenticated namespace,
// not that alias, owns mutation receipts.
pub(super) fn mutation_owner(caller: &ResourceCaller) -> String {
    caller
        .namespace
        .as_ref()
        .map(|resolver| resolver.library_recipe_owner_key())
        .unwrap_or_else(|| format!("provider:{}", caller.provider_id))
}

/// A digest of a request's meaningful fields, length-prefixed so parts cannot run together.
pub(super) fn request_hash(parts: &[&str]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize().into()
}

/// Hashes a request containing a potentially large binary field without expanding it into text.
pub(super) fn request_hash_with_bytes(parts: &[&str], bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    for part in parts {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part.as_bytes());
    }
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
    hash.finalize().into()
}

/// The idempotency key of a mutation, after the shape checks every mutation shares.
pub(super) fn mutation_key(mutation: &MutationContext) -> Result<&str, Status> {
    if mutation.operation_id.len() > MAX_KEY_BYTES {
        return Err(Status::invalid_argument("operation_id is too long"));
    }
    let key = mutation.idempotency_key.as_str();
    if key.is_empty() || key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "idempotency_key is required: 1 to 128 bytes without control characters",
        ));
    }
    Ok(key)
}

/// The display name a request sets (`Some`) or clears (`None`, from an empty value).
fn display_name(fields: &HashMap<String, String>) -> Result<Option<String>, Status> {
    if fields.is_empty() {
        return Err(Status::invalid_argument(
            "display_fields is empty; the writable field is display_name",
        ));
    }
    if let Some(other) = fields.keys().find(|key| key.as_str() != DISPLAY_NAME) {
        return Err(Status::invalid_argument(format!(
            "display_fields.{other} is not writable; the writable field is display_name"
        )));
    }
    let value = fields[DISPLAY_NAME].trim();
    if value.len() > MAX_DISPLAY_NAME_BYTES {
        return Err(Status::invalid_argument(
            "display_name is longer than 256 bytes",
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(Status::invalid_argument(
            "display_name contains control characters",
        ));
    }
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

pub(super) fn game_id(text: &str) -> Result<Option<u64>, Status> {
    match text {
        "" => Ok(None),
        text => text
            .parse::<u64>()
            .map(Some)
            .map_err(|_| Status::invalid_argument("invalid game_id")),
    }
}

pub(super) fn reply(resource_id: &str, revision: u64, outcome: MutationOutcome) -> MutationReply {
    MutationReply {
        resource_id: resource_id.to_owned(),
        revision,
        outcome: outcome as i32,
    }
}

impl ResourceApi {
    /// Fails unless `profile_id` is empty or names the active account profile.
    fn check_profile(&self, profile_id: &str) -> Result<(), Status> {
        if profile_id.is_empty() {
            return Ok(());
        }
        match self.providers.profiles().current() {
            Some(profile) if profile.profile_id() == profile_id => Ok(()),
            _ => Err(Status::failed_precondition(
                "profile_id is not the active profile",
            )),
        }
    }

    /// A commit-time check for `permission` that also fails when the grants changed since the
    /// call was authorized.
    pub(super) fn guard(
        &self,
        context: ServiceContext,
        permission: &'static str,
        grant_revision: u64,
    ) -> CommitGuard {
        let providers = self.providers.clone();
        Box::new(move || {
            let current = providers.authorize_resource(Some(&context), permission)?;
            if current.grant_revision == grant_revision {
                Ok(())
            } else {
                Err(Status::failed_precondition(
                    "plugin permissions changed; try again",
                ))
            }
        })
    }

    pub(super) async fn update_metadata(
        &self,
        request: Request<MetadataUpdateRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::LIBRARY_METADATA_WRITE)?;
        let key = mutation_key(mutation)?;
        let name = display_name(&request.display_fields)?;
        let expected_game = game_id(&request.game_id)?;
        if request.installation_id.is_empty() {
            return Err(Status::invalid_argument("installation_id is required"));
        }
        let hash = request_hash(&[
            "metadata",
            &request.installation_id,
            &request.game_id,
            &mutation.expected_revision.to_string(),
            name.as_deref().unwrap_or("\u{0}"),
        ]);

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(original));
        }
        // Writes never reach another provider's entries, whatever the caller may read.
        let own = Scope {
            own_provider: caller.provider_id.clone(),
            read_all: false,
            provider_filter: None,
        };
        let (entry_key, entry) = self
            .library
            .target(&own, &request.installation_id)
            .filter(|(_, entry)| expected_game.is_none() || entry.game_id == expected_game)
            .ok_or_else(|| Status::not_found("library entry not found"))?;
        let host = self.library.host().ok_or_else(unavailable)?;
        let guard = self.guard(
            mutation.context.clone().unwrap_or_default(),
            cap::LIBRARY_METADATA_WRITE,
            caller.grant_revision,
        );
        let written = host
            .set_display_name(entry_key, mutation.expected_revision, name, guard)
            .await?;
        let (outcome, revision, commit) = match written {
            DisplayWrite::Applied { revision } => (MutationOutcome::Applied, revision, true),
            DisplayWrite::Unchanged { revision } => {
                (MutationOutcome::AlreadyApplied, revision, true)
            }
            DisplayWrite::Conflict { current } => {
                (MutationOutcome::RevisionConflict, current, false)
            }
        };
        let answer = reply(&entry.installation_id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &answer);
        }
        Ok(Response::new(answer))
    }

    pub(super) async fn set_favorite(
        &self,
        request: Request<FavoritesSetRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::FAVORITES_WRITE)?;
        let key = mutation_key(mutation)?;
        if mutation.expected_revision != 0 {
            return Err(Status::invalid_argument(
                "favorites carry no revision; expected_revision must be 0",
            ));
        }
        self.check_profile(&request.profile_id)?;
        let game = game_id(&request.game_id)?
            .ok_or_else(|| Status::invalid_argument("game_id is required"))?;
        let hash = request_hash(&[
            "favorite",
            &game.to_string(),
            if request.favorite { "1" } else { "0" },
        ]);

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(original));
        }
        let host = self.library.host().ok_or_else(unavailable)?;
        let guard = self.guard(
            mutation.context.clone().unwrap_or_default(),
            cap::FAVORITES_WRITE,
            caller.grant_revision,
        );
        let outcome =
            match host.set_favorite(&caller.provider_id, game, request.favorite, guard.as_ref())? {
                FavoriteWrite::Applied => MutationOutcome::Applied,
                FavoriteWrite::AlreadyApplied => MutationOutcome::AlreadyApplied,
                // The same answer for an unknown game and for one only another provider has.
                FavoriteWrite::NoMatchedEntry => {
                    return Err(Status::not_found(
                        "none of this provider's entries is matched to that game",
                    ));
                }
            };
        let answer = reply(&game.to_string(), 0, outcome);
        self.mutations.remember(&caller, key, hash, &answer);
        Ok(Response::new(answer))
    }

    pub(super) async fn list_favorites(
        &self,
        request: Request<FavoritesListRequest>,
    ) -> Result<Response<FavoritesListReply>, Status> {
        let request = request.into_inner();
        let page = request
            .page
            .ok_or_else(|| Status::invalid_argument("page is required"))?;
        let caller = self
            .providers
            .authorize_resource(page.context.as_ref(), cap::FAVORITES_READ)?;
        no_common_data(page.common_data_categories.as_ref())?;
        self.check_profile(&request.profile_id)?;
        let limit = match page.page_size as usize {
            0 => DEFAULT_PAGE_SIZE,
            size => size.min(MAX_PAGE_SIZE),
        };
        let binding = CursorBinding {
            route: "favorites",
            session_id: caller.session_id.clone(),
            profile_epoch: caller.profile_epoch,
            grant_revision: caller.grant_revision,
            scope: Scope {
                own_provider: caller.provider_id.clone(),
                read_all: false,
                provider_filter: None,
            },
            search: None,
        };
        let resumed = if page.cursor.is_empty() {
            None
        } else {
            let (after, issued) =
                self.library
                    .open_cursor(&page.cursor, &binding)
                    .map_err(|error| match error {
                        CursorError::Malformed => {
                            Status::invalid_argument("invalid favorites cursor")
                        }
                        CursorError::Stale => stale_cursor(),
                    })?;
            let after = after
                .parse::<u64>()
                .map_err(|_| Status::invalid_argument("invalid favorites cursor"))?;
            Some((after, issued))
        };
        let host = self.library.host().ok_or_else(unavailable)?;
        let revision = self.library.revision();
        if resumed.is_some_and(|(_, issued)| issued != revision) {
            return Err(stale_cursor());
        }
        let mut games = host.favorites(&caller.provider_id);
        games.sort_unstable_by_key(|(game, _)| *game);
        let after = resumed.map(|(after, _)| after);
        let mut remaining = games
            .into_iter()
            .filter(|(game, _)| after.is_none_or(|after| *game > after));
        let entries: Vec<_> = remaining.by_ref().take(limit).collect();
        let next_cursor = match entries.last() {
            Some((last, _)) if remaining.next().is_some() => {
                self.library
                    .issue_cursor(&binding, revision, &last.to_string())
            }
            _ => String::new(),
        };
        Ok(Response::new(FavoritesListReply {
            entries: entries
                .into_iter()
                .map(|(game, favorite)| FavoriteEntry {
                    game_id: game.to_string(),
                    favorite,
                    revision: 0,
                })
                .collect(),
            next_cursor,
            revision,
        }))
    }
}

pub(super) fn unavailable() -> Status {
    Status::unavailable("the library is not available")
}
