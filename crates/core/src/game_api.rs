//! The GameNode game endpoints the library needs, as a port implemented by `gamenode-api`.
//!
//! Every call takes the session credential and never retries. A `401` is reported as
//! [`ApiError::Rejected`] so the caller can send it down the same path as a rejected session check;
//! every other failure (network, timeout, any other status, an unreadable answer) is
//! [`ApiError::Failed`], and a failed call must never change stored state.

use async_trait::async_trait;

use crate::credential::SessionCredential;
use crate::library::cover::CoverUrl;
use crate::library::membership::EntryStatus;

/// The largest cover that is downloaded.
pub const MAX_COVER_BYTES: usize = 5 * 1024 * 1024;

/// Why a cover was not downloaded. All of them are kept as "failed" and retried on a later scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CoverError {
    #[error("the cover download failed")]
    Failed,
    #[error("the cover is larger than the allowed size")]
    TooLarge,
    #[error("the cover is not a JPEG image")]
    NotAnImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    /// The server refused the session (`401`).
    #[error("GameNode rejected the session")]
    Rejected,
    /// The request could not be completed or its answer could not be used.
    #[error("the GameNode request failed")]
    Failed,
}

/// A GameNode game as the library shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameSummary {
    pub id: u64,
    pub name: String,
    pub release_year: Option<i32>,
    /// The server's cover URL, exactly as sent. It is checked before any download.
    pub cover_url: Option<String>,
}

/// One answer of the external-game lookup: this store id is this GameNode game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalMatch {
    pub uid: String,
    pub game: GameSummary,
}

/// The caller's entry for a game, as GameNode has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LibraryEntry {
    /// `None` when GameNode sent a status this build does not know. The game is still in the
    /// library.
    pub status: Option<EntryStatus>,
}

/// One of the user's GameNode collections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Collection {
    pub id: String,
    pub name: String,
    /// When set, GameNode gives every entry added to this collection this status, whatever
    /// status the request carries.
    pub default_status: Option<EntryStatus>,
}

#[async_trait]
pub trait GameApi: Send + Sync {
    /// Looks up store ids of one external category (`GET /v1/game/external`).
    ///
    /// The implementation splits `uids` into requests within the server's limits and follows
    /// every page. One id can map to several games; a game is listed once per id. An id GameNode
    /// does not know simply has no entry in the answer. If any request fails the whole lookup
    /// fails, so a partial answer is never taken for a complete one.
    async fn lookup_external(
        &self,
        credential: &SessionCredential,
        category: u32,
        uids: &[String],
    ) -> Result<Vec<ExternalMatch>, ApiError>;

    /// Searches GameNode's games by title (`POST /v1/search/games`). An empty answer is `Ok`.
    async fn search_games(
        &self,
        credential: &SessionCredential,
        query: &str,
    ) -> Result<Vec<GameSummary>, ApiError>;

    /// Downloads a cover image. Sends no credential: covers are public images.
    ///
    /// At most [`MAX_COVER_BYTES`] are read, redirects are not followed, and the body must be a
    /// JPEG.
    async fn fetch_cover(&self, cover: &CoverUrl) -> Result<Vec<u8>, CoverError>;

    /// The user's collections (`GET /v1/collections/library/:userId`), private ones included.
    async fn list_collections(
        &self,
        credential: &SessionCredential,
        user_id: &str,
    ) -> Result<Vec<Collection>, ApiError>;

    /// The caller's entry for a game (`GET /v1/collections-entries/game/:id`).
    /// `Ok(None)` means the game is not in the library (`404`).
    async fn library_entry(
        &self,
        credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<LibraryEntry>, ApiError>;

    /// Adds a game to the library as a PC game that is not finished
    /// (`POST /v1/collections-entries`).
    ///
    /// The server overwrites an existing entry, so callers must have just checked with
    /// [`GameApi::library_entry`] that there is none.
    async fn create_entry(
        &self,
        credential: &SessionCredential,
        game_id: u64,
        status: EntryStatus,
        collection_ids: &[String],
    ) -> Result<(), ApiError>;
}
