//! The game endpoints: external-id lookup first, more as the library needs them.

use std::collections::BTreeSet;

use async_trait::async_trait;
use gamenode_core::SessionCredential;
use gamenode_core::callback::NATIVE_ORIGIN;
use gamenode_core::game_api::{
    ApiError, Collection, CoverError, ExternalMatch, GameApi, GameSummary, LibraryEntry,
    MAX_COVER_BYTES,
};
use gamenode_core::library::cover::{CoverUrl, artwork_download_url};
use gamenode_core::library::membership::EntryStatus;
use reqwest::{RequestBuilder, StatusCode};
use serde::{Deserialize, Deserializer};
use url::Url;

use crate::{GameNodeClient, NATIVE_ORIGIN_HEADER};

/// The server accepts 1-100 ids per lookup; 50 keeps a page of two games per id in one answer.
const LOOKUP_BATCH: usize = 50;
const LOOKUP_PAGE_SIZE: usize = 100;
/// Ids longer than this are refused by the server and cannot be GameNode store ids.
const MAX_UID_LEN: usize = 256;
/// A lookup of one batch never needs this many pages; it stops a server that keeps saying "more".
const MAX_PAGES: usize = 50;
/// The server refuses longer queries.
const MAX_QUERY_LEN: usize = 256;
/// Enough for the match dialog to list; the server allows up to 100.
const SEARCH_LIMIT: usize = 20;
/// GameNode's platform id for PC.
const PLATFORM_PC: u32 = 6;
/// User ids are UUID-like; anything longer is not a user id and is never put in a URL.
const MAX_USER_ID_LEN: usize = 128;

/// Whether `id` is safe to use as one URL path segment.
fn is_path_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_USER_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl GameNodeClient {
    /// `<base>/v1/<path>`, with the query set by `build`.
    pub(crate) fn v1(
        &self,
        path: &str,
        build: impl FnOnce(&mut url::form_urlencoded::Serializer<'_, url::UrlQuery<'_>>),
    ) -> Url {
        let mut url = self.base.clone();
        let prefix = url.path().trim_end_matches('/').to_owned();
        url.set_path(&format!("{prefix}/v1/{path}"));
        url.set_query(None);
        build(&mut url.query_pairs_mut());
        url
    }

    pub(crate) fn authorized(
        &self,
        request: RequestBuilder,
        credential: &SessionCredential,
    ) -> RequestBuilder {
        request
            .header(NATIVE_ORIGIN_HEADER, NATIVE_ORIGIN)
            .bearer_auth(credential.expose())
    }

    /// Downloads the image at `url`: 200 only, at most [`MAX_COVER_BYTES`], and a JPEG.
    async fn download_image(&self, url: &Url) -> Result<Vec<u8>, CoverError> {
        let mut response = self
            .http
            .get(url.clone())
            .send()
            .await
            .map_err(|_| CoverError::Failed)?;
        if response.status() != StatusCode::OK {
            tracing::warn!(
                status = response.status().as_u16(),
                "a game image was refused"
            );
            return Err(CoverError::Failed);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_COVER_BYTES as u64)
        {
            return Err(CoverError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| CoverError::Failed)? {
            // A server can lie about, or omit, the length: the cap is enforced on what arrives.
            if body.len() + chunk.len() > MAX_COVER_BYTES {
                return Err(CoverError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        if !body.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Err(CoverError::NotAnImage);
        }
        Ok(body)
    }

    async fn artwork_for_game(
        &self,
        credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<Vec<u8>>, ApiError> {
        let url = self.v1(&format!("game/repository/{game_id}"), |_| {});
        let response = self
            .authorized(self.http.post(url), credential)
            .json(&serde_json::json!({ "relations": { "artworks": true } }))
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => return Err(ApiError::Rejected),
            StatusCode::NOT_FOUND => return Ok(None),
            status => {
                tracing::warn!(
                    status = status.as_u16(),
                    "the game artwork lookup was refused"
                );
                return Err(ApiError::Failed);
            }
        }
        let game: GameArtworks = response.json().await.map_err(|_| ApiError::Failed)?;
        let artwork = game
            .artworks
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| {
                let url = artwork_download_url(item.url.as_deref()?)?;
                let width = u64::from(item.width.unwrap_or_default());
                let height = u64::from(item.height.unwrap_or_default());
                Some(((width > height, width * height, width), url))
            })
            .max_by_key(|(quality, _)| *quality)
            .map(|(_, url)| url);
        let Some(artwork) = artwork else {
            return Ok(None);
        };
        match self.download_image(artwork.as_url()).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(_) => Ok(None),
        }
    }

    async fn lookup_page(
        &self,
        credential: &SessionCredential,
        category: u32,
        uids: &[&str],
        offset: usize,
    ) -> Result<LookupPage, ApiError> {
        let url = self.v1("game/external", |query| {
            query.append_pair("category", &category.to_string());
            for uid in uids {
                query.append_pair("uids", uid);
            }
            query.append_pair("limit", &LOOKUP_PAGE_SIZE.to_string());
            query.append_pair("offset", &offset.to_string());
        });
        let response = self
            .authorized(self.http.get(url), credential)
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => return Err(ApiError::Rejected),
            status => {
                tracing::warn!(
                    status = status.as_u16(),
                    "the external-game lookup was refused"
                );
                return Err(ApiError::Failed);
            }
        }
        response.json().await.map_err(|_| ApiError::Failed)
    }
}

#[derive(Deserialize)]
struct LookupPage {
    data: Vec<LookupRow>,
    pagination: Pagination,
}

#[derive(Deserialize)]
struct Pagination {
    #[serde(rename = "hasNextPage", default)]
    has_next_page: bool,
}

#[derive(Deserialize)]
struct LookupRow {
    uid: String,
    #[serde(rename = "gameId", deserialize_with = "number_or_string")]
    game_id: u64,
    /// The store's own year for the game, a fallback for the GameNode release date.
    #[serde(default)]
    year: Option<i32>,
    #[serde(default)]
    game: Option<LookupGame>,
}

#[derive(Deserialize)]
struct LookupGame {
    #[serde(default)]
    name: Option<String>,
    #[serde(rename = "firstReleaseDate", default)]
    first_release_date: Option<String>,
    #[serde(default)]
    cover: Option<LookupCover>,
}

#[derive(Deserialize)]
struct LookupCover {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Deserialize)]
struct GameArtworks {
    #[serde(default)]
    artworks: Option<Vec<GameArtwork>>,
}

#[derive(Deserialize)]
struct GameArtwork {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

/// Ids that may arrive as a JSON number or, for 64-bit database columns, as a string.
fn number_or_string<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Number(u64),
        Text(String),
    }
    match Id::deserialize(deserializer)? {
        Id::Number(id) => Ok(id),
        Id::Text(text) => text.trim().parse().map_err(serde::de::Error::custom),
    }
}

/// The year of an ISO date such as `2011-04-18T00:00:00.000Z`.
fn year_of(date: &str) -> Option<i32> {
    date.get(..4)?.parse().ok()
}

impl LookupRow {
    fn into_match(self) -> ExternalMatch {
        let (name, date, cover_url) = match self.game {
            Some(game) => (
                game.name,
                game.first_release_date,
                game.cover.and_then(|cover| cover.url),
            ),
            None => (None, None, None),
        };
        ExternalMatch {
            uid: self.uid,
            game: GameSummary {
                id: self.game_id,
                name: name.unwrap_or_default(),
                release_year: date.as_deref().and_then(year_of).or(self.year),
                cover_url,
            },
        }
    }
}

#[derive(Deserialize)]
struct SearchResponse {
    data: SearchData,
}

#[derive(Deserialize)]
struct SearchData {
    items: Vec<SearchItem>,
    #[serde(default, rename = "hasNextPage")]
    has_next_page: Option<bool>,
    #[serde(default)]
    total: Option<usize>,
}

#[derive(Deserialize)]
struct SearchItem {
    #[serde(deserialize_with = "number_or_string")]
    id: u64,
    name: String,
    #[serde(rename = "firstReleaseDate", default)]
    first_release_date: Option<String>,
    #[serde(rename = "coverUrl", default)]
    cover_url: Option<String>,
}

#[derive(Deserialize)]
struct CollectionRow {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(rename = "defaultEntryStatus", default)]
    default_entry_status: Option<String>,
}

#[derive(Deserialize)]
struct EntryRow {
    status: String,
    #[serde(default)]
    collections: Vec<IdRow>,
    #[serde(rename = "ownedPlatforms", default)]
    owned_platforms: Vec<PlatformRow>,
    #[serde(rename = "finishedAt", default)]
    finished_at: Option<String>,
}
#[derive(Deserialize)]
struct IdRow {
    id: String,
}
#[derive(Deserialize)]
struct PlatformRow {
    id: u64,
}

#[async_trait]
impl GameApi for GameNodeClient {
    async fn lookup_external(
        &self,
        credential: &SessionCredential,
        category: u32,
        uids: &[String],
    ) -> Result<Vec<ExternalMatch>, ApiError> {
        let unique: BTreeSet<&str> = uids
            .iter()
            .map(String::as_str)
            .filter(|uid| !uid.is_empty() && uid.len() <= MAX_UID_LEN)
            .collect();
        let unique: Vec<&str> = unique.into_iter().collect();

        let mut seen = BTreeSet::new();
        let mut found = Vec::new();
        for batch in unique.chunks(LOOKUP_BATCH) {
            let mut offset = 0;
            for _ in 0..MAX_PAGES {
                let page = self
                    .lookup_page(credential, category, batch, offset)
                    .await?;
                let empty = page.data.is_empty();
                for row in page.data {
                    // Rows for ids we did not ask about are ignored.
                    if batch.contains(&row.uid.as_str())
                        && seen.insert((row.uid.clone(), row.game_id))
                    {
                        found.push(row.into_match());
                    }
                }
                if !page.pagination.has_next_page || empty {
                    break;
                }
                offset += LOOKUP_PAGE_SIZE;
            }
        }
        Ok(found)
    }

    async fn search_games(
        &self,
        credential: &SessionCredential,
        query: &str,
    ) -> Result<Vec<GameSummary>, ApiError> {
        Ok(self.search_page(credential, query).await?.games)
    }
    async fn search_page(
        &self,
        credential: &SessionCredential,
        query: &str,
    ) -> Result<gamenode_core::game_api::SearchPage, ApiError> {
        let query: String = query.trim().chars().take(MAX_QUERY_LEN).collect();
        if query.is_empty() {
            return Ok(gamenode_core::game_api::SearchPage {
                games: vec![],
                complete: Some(true),
            });
        }
        let response = self
            .authorized(self.http.post(self.v1("search/games", |_| {})), credential)
            .json(&serde_json::json!({ "query": query, "limit": SEARCH_LIMIT, "page": 1 }))
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED => {}
            StatusCode::UNAUTHORIZED => return Err(ApiError::Rejected),
            status => {
                tracing::warn!(status = status.as_u16(), "the game search was refused");
                return Err(ApiError::Failed);
            }
        }
        let body: SearchResponse = response.json().await.map_err(|_| ApiError::Failed)?;
        let count = body.data.items.len();
        let complete = if count >= SEARCH_LIMIT {
            Some(false)
        } else {
            body.data
                .has_next_page
                .map(|more| !more)
                .or_else(|| body.data.total.map(|total| total <= count))
        };
        let games = body
            .data
            .items
            .into_iter()
            .map(|item| GameSummary {
                id: item.id,
                name: item.name,
                release_year: item.first_release_date.as_deref().and_then(year_of),
                cover_url: item.cover_url.filter(|url| !url.is_empty()),
            })
            .collect();
        Ok(gamenode_core::game_api::SearchPage { games, complete })
    }

    async fn fetch_cover(&self, cover: &CoverUrl) -> Result<Vec<u8>, CoverError> {
        self.download_image(cover.as_url()).await
    }

    async fn fetch_artwork(
        &self,
        credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<Vec<u8>>, ApiError> {
        self.artwork_for_game(credential, game_id).await
    }

    async fn list_collections(
        &self,
        credential: &SessionCredential,
        user_id: &str,
    ) -> Result<Vec<Collection>, ApiError> {
        if !is_path_safe_id(user_id) {
            return Err(ApiError::Failed);
        }
        let response = self
            .authorized(
                self.http
                    .get(self.v1(&format!("collections/library/{user_id}"), |_| {})),
                credential,
            )
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => return Err(ApiError::Rejected),
            status => {
                tracing::warn!(status = status.as_u16(), "the collections list was refused");
                return Err(ApiError::Failed);
            }
        }
        let rows: Vec<CollectionRow> = response.json().await.map_err(|_| ApiError::Failed)?;
        Ok(rows
            .into_iter()
            .map(|row| Collection {
                name: if row.name.trim().is_empty() {
                    row.id.clone()
                } else {
                    row.name
                },
                // A status this version does not know cannot be shown, so it counts as none.
                default_status: row
                    .default_entry_status
                    .as_deref()
                    .and_then(EntryStatus::from_api_name),
                id: row.id,
            })
            .collect())
    }

    async fn library_entry(
        &self,
        credential: &SessionCredential,
        game_id: u64,
    ) -> Result<Option<LibraryEntry>, ApiError> {
        let response = self
            .authorized(
                self.http
                    .get(self.v1(&format!("collections-entries/game/{game_id}"), |_| {})),
                credential,
            )
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK => {}
            StatusCode::NOT_FOUND => return Ok(None),
            StatusCode::UNAUTHORIZED => return Err(ApiError::Rejected),
            status => {
                tracing::warn!(
                    status = status.as_u16(),
                    "the library entry check was refused"
                );
                return Err(ApiError::Failed);
            }
        }
        // An entry with a status this build does not know is still an entry.
        let row: EntryRow = response.json().await.map_err(|_| ApiError::Failed)?;
        Ok(Some(LibraryEntry {
            status: EntryStatus::from_api_name(&row.status),
            collection_ids: row.collections.into_iter().map(|c| c.id).collect(),
            platform_ids: row.owned_platforms.into_iter().map(|p| p.id).collect(),
            finished_at: row.finished_at,
        }))
    }

    async fn update_entry(
        &self,
        credential: &SessionCredential,
        game_id: u64,
        status: EntryStatus,
        collection_ids: &[String],
        existing: &LibraryEntry,
    ) -> Result<(), ApiError> {
        if existing.platform_ids.is_empty() {
            return Err(ApiError::Failed);
        }
        let response = self
            .authorized(
                self.http.post(self.v1("collections-entries", |_| {})),
                credential,
            )
            .json(&serde_json::json!({
                "collectionIds": collection_ids, "gameId": game_id,
                "platformIds": existing.platform_ids, "status": status.api_name(),
                "finishedAt": existing.finished_at,
            }))
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            StatusCode::UNAUTHORIZED => Err(ApiError::Rejected),
            _ => Err(ApiError::Failed),
        }
    }

    async fn create_entry(
        &self,
        credential: &SessionCredential,
        game_id: u64,
        status: EntryStatus,
        collection_ids: &[String],
    ) -> Result<(), ApiError> {
        let response = self
            .authorized(
                self.http.post(self.v1("collections-entries", |_| {})),
                credential,
            )
            .json(&serde_json::json!({
                "collectionIds": collection_ids,
                "gameId": game_id,
                "platformIds": [PLATFORM_PC],
                "status": status.api_name(),
                "finishedAt": null,
            }))
            .send()
            .await
            .map_err(|_| ApiError::Failed)?;
        match response.status() {
            StatusCode::OK | StatusCode::CREATED => Ok(()),
            StatusCode::UNAUTHORIZED => Err(ApiError::Rejected),
            status => {
                tracing::warn!(status = status.as_u16(), "adding the game was refused");
                Err(ApiError::Failed)
            }
        }
    }
}

#[cfg(test)]
mod tests;
