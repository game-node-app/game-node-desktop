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
        self.send_cover(installation_id, expected_revision, key, image, media_type)
            .await
    }

    /// Restores the game's own cover for an own entry.
    pub async fn clear_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
    ) -> Result<Written, Status> {
        self.send_cover(installation_id, expected_revision, key, Vec::new(), "")
            .await
    }

    async fn send_cover(
        &self,
        installation_id: &str,
        expected_revision: u64,
        key: &str,
        image: Vec<u8>,
        media_type: &str,
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
            }))
            .await?
            .into_inner()
            .try_into()
    }
}
