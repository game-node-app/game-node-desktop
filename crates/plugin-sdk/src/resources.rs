//! Convenience calls for the library resources of API 1.6: library reads, display names,
//! favorites, catalog matching and covers.
//!
//! Each method builds exactly the request a hand-written raw `ResourceHost` call would and
//! returns the host's answer unchanged, so the two are interchangeable; a service test in the
//! desktop repeats every call both ways and compares them. The helpers add no authority: they use
//! the session's `resource_context`, and the host still checks every grant, scope and revision.
//! Errors are the host's own [`tonic::Status`], whose codes the permissions document explains
//! (`NOT_FOUND` for a foreign or unknown entry, `FAILED_PRECONDITION` for a choice the user made,
//! `RESOURCE_EXHAUSTED` for a rate or byte budget, `UNAVAILABLE` when GameNode cannot answer).
//!
//! # Writing safely
//!
//! Every write needs an idempotency key and, for entries, the revision you last read:
//!
//! * Make one key per intended change with [`idempotency_key`] and **reuse it for retries** of that
//!   change after a timeout or lost connection; the host then returns the original outcome
//!   instead of applying it twice. A new change gets a new key.
//! * [`Outcome::RevisionConflict`] is a normal answer, not an error: the entry changed since you
//!   read it. Read it again ([`Resources::library_get`]) and decide whether the change still
//!   applies, using the new revision. The host does not remember conflicts, so the key may be
//!   reused with the new revision.
//! * [`Outcome::AlreadyApplied`] means the entry already had that value; nothing was written.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tonic::{Request, Status, Streaming};

use crate::proto::resource_host_client::ResourceHostClient;
use crate::proto::*;
use crate::{MAX_MESSAGE_BYTES, SERVICE_TIMEOUT};

/// The most entries the host returns in one page.
pub const MAX_PAGE_SIZE: u32 = 200;
/// How many times [`Resources::library_entries`] and [`Resources::favorite_games`] restart a
/// list the library changed under them before giving up.
const LIST_RESTARTS: usize = 3;
/// The only display field a plugin may write.
const DISPLAY_NAME: &str = "display_name";

/// A consistent walk of the library: the entries and the library revision they were read at,
/// which is where [`Resources::library_watch`] should resume from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibrarySnapshot {
    pub entries: Vec<LibraryEntry>,
    pub revision: u64,
}

/// What a write did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The value changed. `Written::revision` is the new revision.
    Applied,
    /// The entry already had this value; nothing was written.
    AlreadyApplied,
    /// The entry changed since the revision you gave; nothing was written and
    /// `Written::revision` is the current one.
    RevisionConflict,
}

/// The host's answer to a write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    /// The installation id (or the game id, for favorites) the write was about.
    pub resource_id: String,
    /// The resource's revision after the call, or the current one after a conflict.
    pub revision: u64,
    pub outcome: Outcome,
}

/// A proposed, typed custom launch recipe. Handles are opaque host-issued resource identifiers;
/// they are never interpreted by the SDK as filesystem paths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomLaunchRecipe {
    pub game_id: String,
    pub installation_id: String,
    pub expected_revision: u64,
    pub executable_handle: String,
    pub uri: String,
    pub argv: Vec<String>,
    pub working_directory_handle: String,
    pub environment_set: std::collections::BTreeMap<String, String>,
    pub environment_unset: Vec<String>,
    pub runtime_handle: String,
}

impl Written {
    /// Whether the entry now has the requested value, whether this call set it or not.
    pub fn is_done(&self) -> bool {
        matches!(self.outcome, Outcome::Applied | Outcome::AlreadyApplied)
    }
}

impl TryFrom<MutationReply> for Written {
    type Error = Status;

    fn try_from(reply: MutationReply) -> Result<Self, Status> {
        let outcome = match MutationOutcome::try_from(reply.outcome) {
            Ok(MutationOutcome::Applied) => Outcome::Applied,
            Ok(MutationOutcome::AlreadyApplied) => Outcome::AlreadyApplied,
            Ok(MutationOutcome::RevisionConflict) => Outcome::RevisionConflict,
            _ => return Err(Status::internal("the host sent an unknown outcome")),
        };
        Ok(Self {
            resource_id: reply.resource_id,
            revision: reply.revision,
            outcome,
        })
    }
}

/// A cover read back from the host.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cover {
    pub bytes: Vec<u8>,
    /// `image/jpeg` or `image/png`.
    pub media_type: String,
    /// The entry's artwork revision: 0 while the cover is the game's own, not a plugin's.
    pub revision: u64,
}

/// A key for one intended change. Reuse it when retrying that change; use a new one for the
/// next. At most 128 bytes and free of control characters, as the host requires.
pub fn idempotency_key() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    format!(
        "sdk-{:x}-{nanos:x}-{:x}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub(crate) fn timed<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.set_timeout(SERVICE_TIMEOUT);
    request
}

/// Typed access to the library resources, bound to one session's `resource_context`. Get it from
/// [`HostClient::resources`](crate::HostClient::resources). Cheap to clone.
#[derive(Clone)]
pub struct Resources {
    pub(crate) client: ResourceHostClient<tonic::transport::Channel>,
    pub(crate) context: ServiceContext,
}

impl Resources {
    pub(crate) fn new(channel: tonic::transport::Channel, context: ServiceContext) -> Self {
        Self {
            client: ResourceHostClient::new(channel)
                .max_decoding_message_size(MAX_MESSAGE_BYTES)
                .max_encoding_message_size(MAX_MESSAGE_BYTES),
            context,
        }
    }

    pub(crate) fn mutation(&self, key: &str, expected_revision: u64) -> MutationContext {
        MutationContext {
            context: Some(self.context.clone()),
            operation_id: String::new(),
            idempotency_key: key.to_owned(),
            expected_revision,
        }
    }

    fn custom_launch_proposal(
        context: &ServiceContext,
        recipe: &CustomLaunchRecipe,
        user_intent_id: &str,
        key: &str,
    ) -> CustomLaunchRequest {
        CustomLaunchRequest {
            context: Some(context.clone()),
            game_id: recipe.game_id.clone(),
            installation_id: recipe.installation_id.clone(),
            expected_revision: recipe.expected_revision,
            user_intent_id: user_intent_id.to_owned(),
            idempotency_key: key.to_owned(),
            executable_handle: recipe.executable_handle.clone(),
            argv: recipe.argv.clone(),
            working_directory_handle: recipe.working_directory_handle.clone(),
            environment_set: recipe.environment_set.clone().into_iter().collect(),
            environment_unset: recipe.environment_unset.clone(),
            runtime_handle: recipe.runtime_handle.clone(),
            recipe_id: String::new(),
            recipe_revision: 0,
            uri: recipe.uri.clone(),
        }
    }

    fn custom_launch_execution(
        context: &ServiceContext,
        game_id: &str,
        installation_id: &str,
        expected_revision: u64,
        recipe_id: &str,
        recipe_revision: u64,
        user_intent_id: &str,
        key: &str,
    ) -> CustomLaunchRequest {
        CustomLaunchRequest {
            context: Some(context.clone()),
            game_id: game_id.to_owned(),
            installation_id: installation_id.to_owned(),
            expected_revision,
            user_intent_id: user_intent_id.to_owned(),
            idempotency_key: key.to_owned(),
            executable_handle: String::new(),
            argv: Vec::new(),
            working_directory_handle: String::new(),
            environment_set: Default::default(),
            environment_unset: Vec::new(),
            runtime_handle: String::new(),
            recipe_id: recipe_id.to_owned(),
            recipe_revision,
            uri: String::new(),
        }
    }

    fn custom_launch_removal(
        context: &ServiceContext,
        game_id: &str,
        recipe_id: &str,
        recipe_revision: u64,
        key: &str,
    ) -> CustomLaunchRemoveRequest {
        CustomLaunchRemoveRequest {
            mutation: Some(MutationContext {
                context: Some(context.clone()),
                operation_id: String::new(),
                idempotency_key: key.to_owned(),
                expected_revision: recipe_revision,
            }),
            game_id: game_id.to_owned(),
            recipe_id: recipe_id.to_owned(),
        }
    }

    fn page(&self, page_size: u32, cursor: &str) -> PageRequest {
        PageRequest {
            context: Some(self.context.clone()),
            cursor: cursor.to_owned(),
            page_size,
            common_data_categories: None,
        }
    }

    // ---- Library reads (`library.read`, or `library.read.all` for other providers) ----

    /// One page of the library. An empty `provider_id` means the caller's own provider; naming
    /// another needs `library.read.all`. Pass the previous reply's `next_cursor` to continue; a
    /// cursor goes stale when the library changes (`FAILED_PRECONDITION`), and the list then
    /// restarts. Most plugins want [`library_entries`](Self::library_entries).
    pub async fn library_page(
        &self,
        provider_id: &str,
        search: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<LibraryListReply, Status> {
        self.client
            .clone()
            .library_list(timed(LibraryListRequest {
                page: Some(self.page(page_size, cursor)),
                provider_id: provider_id.to_owned(),
                search: search.to_owned(),
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Every entry of `provider_id` (empty: the caller's own), following the cursors. See
    /// [`library_snapshot`](Self::library_snapshot).
    pub async fn library_entries(&self, provider_id: &str) -> Result<Vec<LibraryEntry>, Status> {
        self.library_snapshot(provider_id)
            .await
            .map(|snapshot| snapshot.entries)
    }

    /// Every entry of `provider_id` (empty: the caller's own) and the library revision they were
    /// read at, following the cursors. If the library changes during the walk the list restarts,
    /// up to three times, so the result is one consistent snapshot; after that the last
    /// `FAILED_PRECONDITION` is returned. Resume a watch from `revision`.
    pub async fn library_snapshot(&self, provider_id: &str) -> Result<LibrarySnapshot, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut entries = Vec::new();
            let mut cursor = String::new();
            loop {
                match self
                    .library_page(provider_id, "", MAX_PAGE_SIZE, &cursor)
                    .await
                {
                    Ok(page) => {
                        entries.extend(page.entries);
                        if page.next_cursor.is_empty() {
                            return Ok(LibrarySnapshot {
                                entries,
                                revision: page.revision,
                            });
                        }
                        cursor = page.next_cursor;
                    }
                    // Only a cursor can go stale; a refusal of the first page is a real error.
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// One of the caller's own entries by its `installation_id`.
    pub async fn library_get(&self, installation_id: &str) -> Result<LibraryEntry, Status> {
        let reply = self
            .client
            .clone()
            .library_get(timed(LibraryGetRequest {
                context: Some(self.context.clone()),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                common_data_categories: None,
            }))
            .await?
            .into_inner();
        reply
            .entry
            .ok_or_else(|| Status::internal("the host sent no entry"))
    }

    /// Library changes after `after_revision` (0: from now on), as the host's own event stream.
    /// A position the host no longer retains is `FAILED_PRECONDITION`: list again, then watch
    /// from the list's revision.
    pub async fn library_watch(
        &self,
        provider_id: &str,
        after_revision: u64,
    ) -> Result<Streaming<LibraryEvent>, Status> {
        self.client
            .clone()
            .library_watch(LibraryWatchRequest {
                context: Some(self.context.clone()),
                after_revision,
                provider_id: provider_id.to_owned(),
                common_data_categories: None,
            })
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Display name (`library.metadata.write`) ----

    /// Sets the display name of an own entry, or restores the detected name with `None`.
    /// `expected_revision` is the entry's `metadata_revision`.
    pub async fn set_display_name(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        display_name: Option<&str>,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .metadata_update(timed(MetadataUpdateRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                display_fields: [(
                    DISPLAY_NAME.to_owned(),
                    display_name.unwrap_or_default().to_owned(),
                )]
                .into(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Favorites (`favorites.read`, `favorites.write`) ----

    /// The favorite state of every GameNode game matched by the caller's own entries, as
    /// `(game_id, favorite)`, following the cursors like [`library_entries`](Self::library_entries).
    pub async fn favorite_games(&self) -> Result<Vec<(String, bool)>, Status> {
        let mut restarts = 0;
        'again: loop {
            let mut games = Vec::new();
            let mut cursor = String::new();
            loop {
                let reply = self
                    .client
                    .clone()
                    .favorites_list(timed(FavoritesListRequest {
                        page: Some(self.page(MAX_PAGE_SIZE, &cursor)),
                        profile_id: String::new(),
                    }))
                    .await;
                match reply {
                    Ok(page) => {
                        let page = page.into_inner();
                        games.extend(page.entries.into_iter().map(|e| (e.game_id, e.favorite)));
                        if page.next_cursor.is_empty() {
                            return Ok(games);
                        }
                        cursor = page.next_cursor;
                    }
                    Err(status)
                        if status.code() == tonic::Code::FailedPrecondition
                            && !cursor.is_empty()
                            && restarts < LIST_RESTARTS =>
                    {
                        restarts += 1;
                        continue 'again;
                    }
                    Err(status) => return Err(status),
                }
            }
        }
    }

    /// Marks or unmarks a GameNode game that one of the caller's own entries is matched to. A
    /// favorite has no revision: this is a desired-state write.
    pub async fn set_favorite(
        &self,
        game_id: &str,
        favorite: bool,
        key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .favorites_set(timed(FavoritesSetRequest {
                mutation: Some(self.mutation(key, 0)),
                profile_id: String::new(),
                game_id: game_id.to_owned(),
                favorite,
            }))
            .await?
            .into_inner()
            .try_into()
    }

    /// Proposes a recipe for later custom execution. This never calls standard `game.launch` and
    /// does not start a process; the host validates the typed handles and permission separately.
    pub async fn propose_custom_launch(
        &self,
        recipe: &CustomLaunchRecipe,
        user_intent_id: &str,
        key: &str,
    ) -> Result<CustomLaunchReply, Status> {
        self.client
            .clone()
            .games_propose_custom_launch(timed(Self::custom_launch_proposal(
                &self.context,
                recipe,
                user_intent_id,
                key,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Requests execution of a previously proposed recipe revision. The request carries no
    /// replacement executable, argv, cwd, runtime or environment payload, so the host can only
    /// execute the immutable recipe it approved. A denied `game.custom_launch` grant stays a
    /// `PERMISSION_DENIED` error; there is no fallback to standard launch.
    pub async fn request_custom_launch(
        &self,
        game_id: &str,
        installation_id: &str,
        expected_revision: u64,
        recipe_id: &str,
        recipe_revision: u64,
        user_intent_id: &str,
        key: &str,
    ) -> Result<CustomLaunchReply, Status> {
        self.client
            .clone()
            .games_request_custom_launch(timed(Self::custom_launch_execution(
                &self.context,
                game_id,
                installation_id,
                expected_revision,
                recipe_id,
                recipe_revision,
                user_intent_id,
                key,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Removes one of this caller's recipes at the revision it last read.
    pub async fn remove_custom_launch(
        &self,
        game_id: &str,
        recipe_id: &str,
        recipe_revision: u64,
        key: &str,
    ) -> Result<MutationReply, Status> {
        self.client
            .clone()
            .games_remove_custom_launch(timed(Self::custom_launch_removal(
                &self.context,
                game_id,
                recipe_id,
                recipe_revision,
                key,
            )))
            .await
            .map(tonic::Response::into_inner)
    }

    // ---- Matching (`matching.read`, `matching.write`) ----

    /// Searches the GameNode catalog by title: one page of at most `page_size` candidates (0: the
    /// host's default of 20; the host caps it at 50), in the server's order. `UNAVAILABLE` when
    /// GameNode cannot answer, never an empty list; `RESOURCE_EXHAUSTED` past the host's rate.
    pub async fn search_catalog(
        &self,
        query: &str,
        page_size: u32,
    ) -> Result<Vec<MatchCandidate>, Status> {
        self.client
            .clone()
            .matching_search(timed(MatchingSearchRequest {
                context: Some(self.context.clone()),
                query: query.to_owned(),
                platform: String::new(),
                page_size,
                cursor: String::new(),
            }))
            .await
            .map(|reply| reply.into_inner().candidates)
    }

    /// Matches an own entry to a GameNode game that the library knows or a recent
    /// [`search_catalog`](Self::search_catalog) returned. `expected_revision` is the entry's
    /// `revision`. A match the user chose is `FAILED_PRECONDITION` and is never replaced.
    pub async fn assign_match(
        &self,
        installation_id: &str,
        gamenode_game_id: &str,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .matching_assign(timed(MatchingAssignRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                provider_id: String::new(),
                provider_game_id: String::new(),
                gamenode_game_id: gamenode_game_id.to_owned(),
                installation_id: installation_id.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Covers (`artwork.read`, `artwork.write`) ----

    /// The cover an own entry shows, at most `max_bytes` (0: the host's 5 MiB maximum). Nothing is
    /// downloaded: an entry whose cover is not on this device is `NOT_FOUND`, and a larger image
    /// is `RESOURCE_EXHAUSTED` rather than truncated.
    pub async fn read_cover(&self, installation_id: &str, max_bytes: u64) -> Result<Cover, Status> {
        let reply = self
            .client
            .clone()
            .artwork_read(timed(ArtworkReadRequest {
                context: Some(self.context.clone()),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                artwork_kind: String::new(),
                max_bytes,
            }))
            .await?
            .into_inner();
        Ok(Cover {
            bytes: reply.encoded_image,
            media_type: reply.media_type,
            revision: reply.revision,
        })
    }

    /// Sets the cover of an own entry from an encoded static PNG, JPEG or WebP (at most 4 MiB and
    /// 4096 x 4096 pixels; `media_type` must match the bytes). The host decodes, shrinks and
    /// re-encodes it; the matched game's cover is not touched. `expected_revision` is the entry's
    /// `artwork_revision`. A refused image is `INVALID_ARGUMENT` with the reason.
    pub async fn set_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        image: Vec<u8>,
        media_type: &str,
    ) -> Result<Written, Status> {
        self.send_cover(
            installation_id,
            expected_revision,
            key,
            image,
            media_type,
            "",
        )
        .await
    }

    /// Like [`set_cover`](Self::set_cover), but the image is a file the plugin may read:
    /// `file_handle` is a regular-file handle from the Files service (`files_list`,
    /// `files_create`), from an approval of this same installation. It needs `files.read` besides
    /// `artwork.write`; the host reads the file itself under the same 4 MiB bound, so the plugin
    /// does not have to hold the bytes. A path or URL is never accepted. `media_type` must match
    /// the file's bytes.
    pub async fn set_cover_from_file(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        file_handle: &str,
        media_type: &str,
    ) -> Result<Written, Status> {
        self.send_cover(
            installation_id,
            expected_revision,
            key,
            Vec::new(),
            media_type,
            file_handle,
        )
        .await
    }

    /// Restores the game's own cover for an own entry.
    pub async fn clear_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.send_cover(installation_id, expected_revision, key, Vec::new(), "", "")
            .await
    }

    async fn send_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        image: Vec<u8>,
        media_type: &str,
        file_handle: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .artwork_set(timed(ArtworkSetRequest {
                mutation: Some(self.mutation(key, expected_revision)),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                artwork_kind: String::new(),
                encoded_image: image,
                media_type: media_type.to_owned(),
                file_handle: file_handle.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }
}

#[cfg(test)]
mod custom_launch_tests {
    use super::*;
    #[test]
    fn proposal_preserves_typed_custom_launch_fields() {
        let context = ServiceContext {
            provider_id: "fixture".into(),
            ..Default::default()
        };
        let recipe = CustomLaunchRecipe {
            game_id: "620".into(),
            installation_id: "steam:620".into(),
            expected_revision: 12,
            executable_handle: "exec-handle".into(),
            uri: "game://open".into(),
            argv: vec!["--literal value".into(), "$(not-a-shell)".into()],
            working_directory_handle: "cwd-handle".into(),
            environment_set: [("A".into(), "value with spaces".into())].into(),
            environment_unset: vec!["B".into()],
            runtime_handle: "runtime-handle".into(),
        };

        let request = Resources::custom_launch_proposal(&context, &recipe, "intent", "key");

        assert_eq!(request.context.as_ref(), Some(&context));
        assert_eq!(request.game_id, "620");
        assert_eq!(request.installation_id, "steam:620");
        assert_eq!(request.expected_revision, 12);
        assert_eq!(request.executable_handle, "exec-handle");
        assert_eq!(request.uri, "game://open");
        assert_eq!(request.argv, recipe.argv);
        assert_eq!(request.working_directory_handle, "cwd-handle");
        assert_eq!(
            request.environment_set.get("A").map(String::as_str),
            Some("value with spaces")
        );
        assert_eq!(request.environment_unset, ["B"]);
        assert_eq!(request.runtime_handle, "runtime-handle");
        assert!(request.recipe_id.is_empty());
        assert_eq!(request.recipe_revision, 0);
        assert_eq!(request.user_intent_id, "intent");
        assert_eq!(request.idempotency_key, "key");
    }

    #[test]
    fn execution_uses_only_the_approved_recipe_reference() {
        let context = ServiceContext::default();
        let request = Resources::custom_launch_execution(
            &context,
            "620",
            "steam:620",
            12,
            "recipe-1",
            4,
            "intent",
            "key",
        );

        assert_eq!(request.context.as_ref(), Some(&context));
        assert_eq!(request.game_id, "620");
        assert_eq!(request.installation_id, "steam:620");
        assert_eq!(request.expected_revision, 12);
        assert_eq!(request.recipe_id, "recipe-1");
        assert_eq!(request.recipe_revision, 4);
        assert_eq!(request.user_intent_id, "intent");
        assert_eq!(request.idempotency_key, "key");
        assert!(request.executable_handle.is_empty());
        assert!(request.uri.is_empty());
        assert!(request.argv.is_empty());
        assert!(request.working_directory_handle.is_empty());
        assert!(request.environment_set.is_empty());
        assert!(request.environment_unset.is_empty());
        assert!(request.runtime_handle.is_empty());
    }

    #[test]
    fn removal_binds_recipe_revision_and_idempotency_key() {
        let context = ServiceContext {
            provider_id: "fixture".into(),
            ..Default::default()
        };
        let request = Resources::custom_launch_removal(&context, "620", "recipe-1", 4, "key");
        let mutation = request.mutation.expect("removal must be revision guarded");

        assert_eq!(request.game_id, "620");
        assert_eq!(request.recipe_id, "recipe-1");
        assert_eq!(mutation.context.as_ref(), Some(&context));
        assert_eq!(mutation.expected_revision, 4);
        assert_eq!(mutation.idempotency_key, "key");
    }
}
