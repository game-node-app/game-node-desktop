//! Convenience calls for the utility services of API 1.6: private storage, files, network fetch
//! and notifications. They are more methods of [`Resources`], with the same guarantees as the
//! library helpers: each builds exactly the request a hand-written raw `ResourceHost` call would
//! and returns the host's answer unchanged, a desktop service test repeats every call both ways,
//! and the helpers add no authority. The host checks every grant, scope, quota and revision.
//!
//! What the services refuse is explained, with every limit and status code, in
//! `docs/plugin-api-permissions.md`. In short, errors are the host's own [`tonic::Status`]:
//!
//! * `PERMISSION_DENIED`: the user did not grant it (or revoked it, or approved other origins);
//! * `FAILED_PRECONDITION`: ask again (file access), or the same key was used for another request;
//! * `NOT_FOUND`: a handle or entry that is not yours or is gone;
//! * `RESOURCE_EXHAUSTED`: a quota, rate or byte budget (nothing was changed or truncated);
//! * `INVALID_ARGUMENT`: the request itself is malformed; do not retry it unchanged;
//! * `UNAVAILABLE`/`DEADLINE_EXCEEDED`: retry with the **same** idempotency key.
//!
//! These services give a plugin what the user approved *through GameNode*. They are not a
//! sandbox: nothing here stops the plugin's own process from using the operating system
//! directly, and a plugin must not claim otherwise.

use std::collections::BTreeMap;
use std::time::Duration;

use tonic::Status;

use crate::proto::*;
use crate::resources::{Resources, Written, timed};

/// How long a stored or shared value is kept and where.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Durability {
    /// Kept in GameNode's memory until the plugin's last session ends. Needs only
    /// `storage.write`.
    SessionOnly,
    /// Survives restarts. Needs `storage.persist`, which the user approves separately; without
    /// it the write is `PERMISSION_DENIED` and is **not** downgraded to a volatile success.
    Persistent,
}

impl Durability {
    /// The wire value to send or compare, shared by storage and shared-data replies.
    pub fn to_proto(self) -> i32 {
        match self {
            Self::SessionOnly => StorageDurability::SessionOnly as i32,
            Self::Persistent => StorageDurability::Persistent as i32,
        }
    }

    /// Reads a wire value. `None` means the field was absent (an older host or client) or the
    /// value is unknown, so the caller never assumes a mode the host did not report.
    pub fn from_proto(value: i32) -> Option<Self> {
        match StorageDurability::try_from(value) {
            Ok(StorageDurability::SessionOnly) => Some(Self::SessionOnly),
            Ok(StorageDurability::Persistent) => Some(Self::Persistent),
            _ => None,
        }
    }
}

/// A key read from storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredValue {
    /// Whether the key exists. An expired value is a key that does not exist.
    pub found: bool,
    pub value: Vec<u8>,
    /// 0 while the key does not exist; the `expected_revision` of the next write or delete.
    pub revision: u64,
    /// Where the value lives now; `None` when it was not found.
    pub durability: Option<Durability>,
    /// Unix seconds; 0 when it never expires.
    pub expires_at: i64,
}

/// What a storage write did, with the layer's usage so a plugin can free space before it runs out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredWrite {
    /// The key's new revision, or the current one after a conflict.
    pub revision: u64,
    pub outcome: crate::Outcome,
    /// Logical bytes in use in the layer that was written, and that layer's quota.
    pub usage_bytes: u64,
    pub quota_bytes: u64,
    pub durability: Option<Durability>,
    pub expires_at: i64,
}

/// One https request for [`Resources::network_fetch`]. Only the URL is required; the host's
/// defaults are a GET with a 1 MiB response budget and a 30 second deadline (both capped).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fetch {
    url: String,
    method: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    max_response_bytes: u64,
    timeout_ms: u32,
}

impl Fetch {
    /// A GET of `url`, which must be https with an origin the user approved.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Self::default()
        }
    }

    /// Another method (`HEAD`, `POST`, `PUT`, `PATCH` or `DELETE`) with an optional body.
    #[must_use]
    pub fn method(mut self, method: &str, body: impl Into<Vec<u8>>) -> Self {
        self.method = method.to_owned();
        self.body = body.into();
        self
    }

    /// Adds a header. The host refuses the ones that are its own to set (`Host`,
    /// `Content-Length`, `Proxy-*`, ...) and adds none of GameNode's.
    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(name.to_owned(), value.to_owned());
        self
    }

    /// The most response bytes to accept; a larger response is `RESOURCE_EXHAUSTED`, not cut.
    #[must_use]
    pub fn max_response_bytes(mut self, bytes: u64) -> Self {
        self.max_response_bytes = bytes;
        self
    }

    /// The deadline for the whole fetch, redirects included.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout_ms = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
        self
    }
}

/// The answer to a fetch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    pub status_code: u16,
    /// Header names are lower case. Bodies are not decompressed.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    /// The origin that finally answered, after redirects.
    pub final_origin: String,
}

/// How a notice is presented.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Severity {
    #[default]
    Unspecified,
    Info,
    Success,
    Warning,
    Error,
}

impl Severity {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "",
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// A short plain-text notice. GameNode shows it as coming from the plugin, picks the translation
/// that matches the user's language, and never interprets anything in it as markup, a link or an
/// action.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notice {
    title: String,
    body: String,
    locale: String,
    severity: Severity,
    translations: Vec<(String, String, String)>,
}

impl Notice {
    /// A notice with a title in `locale` (a language tag such as `en`; empty when unspecified).
    pub fn new(locale: &str, title: &str) -> Self {
        Self {
            title: title.to_owned(),
            locale: locale.to_owned(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn body(mut self, body: &str) -> Self {
        self.body = body.to_owned();
        self
    }

    #[must_use]
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// The same notice in another language. Each `locale` is distinct and non-empty; at most 8.
    #[must_use]
    pub fn translation(mut self, locale: &str, title: &str, body: &str) -> Self {
        self.translations
            .push((locale.to_owned(), title.to_owned(), body.to_owned()));
        self
    }
}

/// A suggestion of where the folder for `kind` (`"save"` or `"prefix"`) usually is. `location` is
/// an absolute path, or starts with a placeholder the host expands: `{home}`, `{documents}`,
/// `{appdata}`, `{localappdata}` (Windows) or `{xdg_data}`, `{xdg_config}` (Linux), followed by
/// plain names separated by `/` (no `..`). `description` is one line of plain text, at most 120
/// bytes. Within one kind, declare the most likely folder first, at most 8 per kind.
pub fn declared_location(kind: &str, location: &str, description: &str) -> DeclaredLocation {
    DeclaredLocation {
        kind: kind.to_owned(),
        location: location.to_owned(),
        description: description.to_owned(),
    }
}

/// The most bytes one `FilesRead` returns.
pub const MAX_READ_CHUNK: u64 = 4 * 1024 * 1024;

impl Resources {
    // ---- Private storage (`storage.read`, `storage.write`, `storage.persist`, `storage.delete`) ----

    /// Reads one key of the plugin's own namespace. A key that does not exist is not an error:
    /// [`StoredValue::found`] is false and `revision` is 0.
    pub async fn storage_get(&self, key: &str) -> Result<StoredValue, Status> {
        let reply = self
            .client
            .clone()
            .storage_get(timed(StorageGetRequest {
                context: Some(self.context.clone()),
                key: key.to_owned(),
            }))
            .await?
            .into_inner();
        Ok(StoredValue {
            found: reply.found,
            value: reply.value,
            revision: reply.revision,
            durability: Durability::from_proto(reply.durability),
            expires_at: reply.expires_at,
        })
    }

    /// Writes one key (at most 1 MiB; the layer holds 16 MiB). `expected_revision` is the key's
    /// revision from [`storage_get`](Self::storage_get), 0 for a new key. `ttl` of `None` keeps
    /// the value until it is deleted; otherwise it expires after `ttl`, which the host limits to
    /// 30 days. A revision conflict is an answer ([`crate::Outcome::RevisionConflict`]), not an
    /// error; a quota failure is `RESOURCE_EXHAUSTED` and changes nothing.
    pub async fn storage_put(
        &self,
        key: &str,
        expected_revision: u64,
        idempotency_key: &str,
        value: impl Into<Vec<u8>>,
        durability: Durability,
        ttl: Option<Duration>,
    ) -> Result<StoredWrite, Status> {
        let reply = self
            .client
            .clone()
            .storage_put(timed(StoragePutRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                key: key.to_owned(),
                value: value.into(),
                durability: durability.to_proto(),
                ttl_seconds: ttl.map_or(0, |ttl| ttl.as_secs().max(1)),
            }))
            .await?
            .into_inner();
        let outcome = Written::try_from(MutationReply {
            resource_id: key.to_owned(),
            revision: reply.revision,
            outcome: reply.outcome,
        })?
        .outcome;
        Ok(StoredWrite {
            revision: reply.revision,
            outcome,
            usage_bytes: reply.usage_bytes,
            quota_bytes: reply.quota_bytes,
            durability: Durability::from_proto(reply.durability),
            expires_at: reply.expires_at,
        })
    }

    /// Deletes one key. Deleting a key that is not there is [`crate::Outcome::AlreadyApplied`].
    pub async fn storage_delete(
        &self,
        key: &str,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .storage_delete(timed(StorageDeleteRequest {
                mutation: Some(self.mutation(idempotency_key, expected_revision)),
                key: key.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Files (`files.read`, `files.write`, `files.delete`) ----

    /// Asks for access to a folder of one of the plugin's own library entries. `operations` are
    /// `"read"`, `"write"` and `"delete"` and `kinds` are `"installation"`, `"save"` or `"prefix"`
    /// (empty: the installation folder). The user chooses and confirms the folder; the plugin never
    /// names a path. The handles are opaque and last for the session: after a restart ask again.
    /// `FAILED_PRECONDITION` and `NOT_FOUND` on a handle mean "ask again".
    pub async fn files_request_access(
        &self,
        installation_id: &str,
        operations: &[&str],
        kinds: &[&str],
        idempotency_key: &str,
    ) -> Result<Vec<FileHandle>, Status> {
        self.files_request_access_with(installation_id, operations, kinds, &[], idempotency_key)
            .await
    }

    /// Like [`files_request_access`](Self::files_request_access), also telling GameNode where the
    /// folder usually is (see [`declared_location`]). The suggestions are shown to the user as
    /// declared by the plugin and not verified; the user takes one or chooses another folder, and
    /// a suggestion alone grants nothing. The reply says nothing about which suggestions exist or
    /// were taken. A malformed suggestion is `INVALID_ARGUMENT`; one that cannot apply on this
    /// system is dropped without an error, so one list can serve every platform.
    pub async fn files_request_access_with(
        &self,
        installation_id: &str,
        operations: &[&str],
        kinds: &[&str],
        declared: &[DeclaredLocation],
        idempotency_key: &str,
    ) -> Result<Vec<FileHandle>, Status> {
        Ok(self
            .client
            .clone()
            .files_request_access(timed(FilesRequestAccessRequest {
                mutation: Some(self.mutation(idempotency_key, 0)),
                game_id: String::new(),
                installation_id: installation_id.to_owned(),
                requested_operations: operations.iter().map(|op| (*op).to_owned()).collect(),
                resource_kinds: kinds.iter().map(|kind| (*kind).to_owned()).collect(),
                declared_locations: declared.to_vec(),
            }))
            .await?
            .into_inner()
            .handles)
    }

    /// One page of a directory, by handle. `page_size` 0 is the host's default.
    pub async fn files_page(
        &self,
        directory: &str,
        page_size: u32,
        cursor: &str,
    ) -> Result<FilesListReply, Status> {
        self.client
            .clone()
            .files_list(timed(FilesListRequest {
                context: Some(self.context.clone()),
                directory_handle: directory.to_owned(),
                cursor: cursor.to_owned(),
                page_size,
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// Every entry of a directory, following the cursors. Each entry has its own handle.
    pub async fn files_list(&self, directory: &str) -> Result<Vec<FileEntry>, Status> {
        let mut entries = Vec::new();
        let mut cursor = String::new();
        loop {
            let page = self.files_page(directory, 0, &cursor).await?;
            entries.extend(page.entries);
            if page.next_cursor.is_empty() {
                return Ok(entries);
            }
            cursor = page.next_cursor;
        }
    }

    /// Up to `max_bytes` (at most [`MAX_READ_CHUNK`]; 0 is the host's default) of a file from
    /// `offset`. Continue at the reply's `next_offset` until `eof`.
    pub async fn files_read(
        &self,
        file: &str,
        offset: u64,
        max_bytes: u64,
    ) -> Result<FileReadReply, Status> {
        self.client
            .clone()
            .files_read(timed(FilesReadRequest {
                context: Some(self.context.clone()),
                file_handle: file.to_owned(),
                offset,
                max_bytes,
            }))
            .await
            .map(tonic::Response::into_inner)
    }

    /// The whole file, read in chunks. A file longer than `limit` bytes is `RESOURCE_EXHAUSTED`
    /// rather than read in part, so the plugin never holds more than it chose to.
    pub async fn files_read_all(&self, file: &str, limit: u64) -> Result<Vec<u8>, Status> {
        let mut contents = Vec::new();
        let mut offset = 0;
        loop {
            let want = limit
                .saturating_sub(contents.len() as u64)
                .saturating_add(1)
                .min(MAX_READ_CHUNK);
            let chunk = self.files_read(file, offset, want).await?;
            contents.extend_from_slice(&chunk.contents);
            if contents.len() as u64 > limit {
                return Err(Status::resource_exhausted(
                    "the file is larger than the limit",
                ));
            }
            if chunk.eof {
                return Ok(contents);
            }
            if chunk.next_offset <= offset {
                return Err(Status::internal("the host did not advance the read"));
            }
            offset = chunk.next_offset;
        }
    }

    /// Writes `contents` into an existing file at `offset` (not past its end); with `truncate`
    /// the file ends where the write does. The reply's `resource_id` is the file's handle.
    pub async fn files_write(
        &self,
        file: &str,
        offset: u64,
        contents: impl Into<Vec<u8>>,
        truncate: bool,
        idempotency_key: &str,
    ) -> Result<Written, Status> {
        self.write(file, "", offset, contents.into(), truncate, idempotency_key)
            .await
    }

    /// Creates, or replaces, the file `name` (one plain name, valid on every platform) inside the
    /// directory `directory`. The reply's `resource_id` is the new file's handle.
    pub async fn files_create(
        &self,
        directory: &str,
        name: &str,
        contents: impl Into<Vec<u8>>,
        idempotency_key: &str,
    ) -> Result<Written, Status> {
        self.write(directory, name, 0, contents.into(), true, idempotency_key)
            .await
    }

    async fn write(
        &self,
        handle: &str,
        name: &str,
        offset: u64,
        contents: Vec<u8>,
        truncate: bool,
        idempotency_key: &str,
    ) -> Result<Written, Status> {
        self.client
            .clone()
            .files_write(timed(FilesWriteRequest {
                mutation: Some(self.mutation(idempotency_key, 0)),
                file_handle: handle.to_owned(),
                offset,
                contents,
                truncate,
                name: name.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    /// Deletes one regular file. Directories are never deleted.
    pub async fn files_delete(&self, file: &str, idempotency_key: &str) -> Result<Written, Status> {
        self.client
            .clone()
            .files_delete(timed(FilesDeleteRequest {
                mutation: Some(self.mutation(idempotency_key, 0)),
                file_handle: file.to_owned(),
            }))
            .await?
            .into_inner()
            .try_into()
    }

    // ---- Network (`network.fetch`) ----

    /// Fetches an https URL for the plugin, from an origin the user approved. The host connects
    /// only to public addresses, follows at most 5 redirects and only to approved origins, adds no
    /// GameNode credential and never reveals where a refused request would have gone. A
    /// non-2xx answer is a normal [`Fetched`]; only a refusal or failure is an error.
    pub async fn network_fetch(&self, fetch: &Fetch) -> Result<Fetched, Status> {
        let reply = self
            .client
            .clone()
            .network_fetch(timed(NetworkFetchRequest {
                context: Some(self.context.clone()),
                url: fetch.url.clone(),
                method: fetch.method.clone(),
                headers: fetch
                    .headers
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect(),
                body: fetch.body.clone(),
                max_response_bytes: fetch.max_response_bytes,
                timeout_ms: fetch.timeout_ms,
            }))
            .await?
            .into_inner();
        Ok(Fetched {
            status_code: u16::try_from(reply.status_code)
                .map_err(|_| Status::internal("the host sent an unknown status code"))?,
            headers: reply.headers.into_iter().collect(),
            body: reply.body,
            final_origin: reply.final_origin,
        })
    }

    // ---- Notifications (`notifications.publish`) ----

    /// Shows a short plain-text notice from the plugin. The host limits a plugin to 5 a minute:
    /// `RESOURCE_EXHAUSTED` carries the wait in `retry-after-ms`, and that attempt is not
    /// remembered, so the same key may be sent again later. Resending a notice the host already
    /// showed (same key, same notice) does not show it twice.
    pub async fn notify(&self, notice: &Notice, idempotency_key: &str) -> Result<Written, Status> {
        self.client
            .clone()
            .notifications_publish(timed(NotificationPublishRequest {
                mutation: Some(self.mutation(idempotency_key, 0)),
                title: notice.title.clone(),
                body: notice.body.clone(),
                locale: notice.locale.clone(),
                severity: notice.severity.as_str().to_owned(),
                translations: notice
                    .translations
                    .iter()
                    .map(|(locale, title, body)| NotificationTranslation {
                        locale: locale.clone(),
                        title: title.clone(),
                        body: body.clone(),
                    })
                    .collect(),
            }))
            .await?
            .into_inner()
            .try_into()
    }
}
