//! Cover read and set of `ResourceHost`.
//!
//! A plugin may set the cover of one of its own library entries, never anyone else's and never
//! the matched game's shared cover: the host keeps the image as an override for that entry
//! (`LibraryEntry.artwork_revision` counts its writes) and an empty `encoded_image` restores the
//! game's cover. The bytes are untrusted, so they are bounded before anything touches them and
//! then decoded, shrunk and re-encoded on a blocking worker (`gamenode_platform::plugin_artwork`):
//! the host stores its own re-encoded picture, never the plugin's file. Reads return the cover
//! the entry shows, already on this device, within the caller's byte budget.

use std::sync::Arc;

use gamenode_core::library::ArtworkWrite;
use gamenode_platform::plugin_artwork::{self, ArtworkError, Prepared};
use gamenode_plugin_protocol::{capability as cap, proto::*};
use sha2::{Digest, Sha256};
use tonic::{Request, Response, Status};

use super::mutations::{game_id, mutation_key, reply, request_hash, unavailable};
use super::{ResourceApi, Scope};
use crate::plugin_host::library::Projected;
use crate::plugin_host::provider::ResourceCaller;

/// The only artwork kind: the cover shown for a library entry.
pub const COVER: &str = "cover";
/// The most a read returns. The covers the host downloads are capped at 5 MiB.
pub(super) const MAX_READ_BYTES: u64 = 5 * 1024 * 1024;
const MAX_INSTALLATION_ID_BYTES: usize = 128;
/// Images decoded at once. A decode may hold ~128 MiB, so two is the most memory the host
/// will spend on plugin pictures at any moment.
pub(super) const DECODE_SLOTS: usize = 2;

fn kind(artwork_kind: &str) -> Result<(), Status> {
    if artwork_kind.is_empty() || artwork_kind == COVER {
        Ok(())
    } else {
        Err(Status::invalid_argument(
            "artwork_kind must be \"cover\" (or empty)",
        ))
    }
}

fn installation(id: &str) -> Result<(), Status> {
    if id.is_empty() {
        return Err(Status::invalid_argument("installation_id is required"));
    }
    if id.len() > MAX_INSTALLATION_ID_BYTES {
        return Err(Status::invalid_argument("installation_id is too long"));
    }
    Ok(())
}

fn refused(error: ArtworkError) -> Status {
    Status::invalid_argument(error.to_string())
}

impl ResourceApi {
    /// The caller's own entry named by `installation_id`, optionally also required to be
    /// matched to `expected_game`. Foreign and unknown entries are the same `NOT_FOUND`.
    pub(super) fn own_entry(
        &self,
        caller: &ResourceCaller,
        installation_id: &str,
        expected_game: Option<u64>,
    ) -> Result<(String, Projected), Status> {
        let own = Scope {
            own_provider: caller.provider_id.clone(),
            read_all: false,
            provider_filter: None,
        };
        self.library
            .target(&own, installation_id)
            .filter(|(_, entry)| expected_game.is_none() || entry.game_id == expected_game)
            .ok_or_else(|| Status::not_found("library entry not found"))
    }

    pub(super) async fn read_artwork(
        &self,
        request: Request<ArtworkReadRequest>,
    ) -> Result<Response<ArtworkReadReply>, Status> {
        let request = request.into_inner();
        let caller = self
            .providers
            .authorize_resource(request.context.as_ref(), cap::ARTWORK_READ)?;
        kind(&request.artwork_kind)?;
        installation(&request.installation_id)?;
        let expected_game = game_id(&request.game_id)?;
        let budget = match request.max_bytes {
            0 => MAX_READ_BYTES,
            bytes => bytes.min(MAX_READ_BYTES),
        };
        let (entry_key, _) = self.own_entry(&caller, &request.installation_id, expected_game)?;
        let host = self.library.host().ok_or_else(unavailable)?;
        let cover = host.read_artwork(entry_key, budget).await?;
        // The bytes were read for a call that may since have lost its permission.
        self.providers
            .authorize_resource(request.context.as_ref(), cap::ARTWORK_READ)?;
        Ok(Response::new(ArtworkReadReply {
            byte_length: cover.bytes.len() as u64,
            encoded_image: cover.bytes,
            media_type: cover.media_type.to_owned(),
            revision: cover.revision,
        }))
    }

    pub(super) async fn set_artwork(
        &self,
        request: Request<ArtworkSetRequest>,
    ) -> Result<Response<MutationReply>, Status> {
        let request = request.into_inner();
        let mutation = request
            .mutation
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("mutation is required"))?;
        let caller = self
            .providers
            .authorize_resource(mutation.context.as_ref(), cap::ARTWORK_WRITE)?;
        let key = mutation_key(mutation)?;
        kind(&request.artwork_kind)?;
        installation(&request.installation_id)?;
        let expected_game = game_id(&request.game_id)?;
        let clearing = request.encoded_image.is_empty();
        if clearing && !request.media_type.is_empty() {
            return Err(Status::invalid_argument(
                "media_type must be empty when encoded_image is empty (restoring the game's cover)",
            ));
        }
        if request.encoded_image.len() > plugin_artwork::MAX_INPUT_BYTES {
            return Err(refused(ArtworkError::TooLarge));
        }
        let content = if clearing {
            "\u{0}".to_owned()
        } else {
            format!(
                "{:x}|{}",
                Sha256::digest(&request.encoded_image),
                request.media_type.trim().to_ascii_lowercase()
            )
        };
        let hash = request_hash(&[
            "artwork",
            &request.installation_id,
            &request.game_id,
            &mutation.expected_revision.to_string(),
            &content,
        ]);

        // A retried call is answered before spending any work on its bytes.
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(original));
        }
        // Nothing is decoded for an entry the caller does not own.
        self.own_entry(&caller, &request.installation_id, expected_game)?;
        let prepared = if clearing {
            None
        } else {
            Some(
                self.prepare_image(request.encoded_image, request.media_type)
                    .await?,
            )
        };

        let _serial = self.mutations.lock.lock().await;
        if let Some(original) = self.mutations.replay(&caller, key, hash)? {
            return Ok(Response::new(original));
        }
        let (entry_key, entry) =
            self.own_entry(&caller, &request.installation_id, expected_game)?;
        let host = self.library.host().ok_or_else(unavailable)?;
        let guard = self.guard(
            mutation.context.clone().unwrap_or_default(),
            cap::ARTWORK_WRITE,
            caller.grant_revision,
        );
        let (outcome, revision, commit) = match host
            .set_artwork(entry_key, mutation.expected_revision, prepared, guard)
            .await?
        {
            ArtworkWrite::Applied { revision, .. } => (MutationOutcome::Applied, revision, true),
            ArtworkWrite::Unchanged { revision } => {
                (MutationOutcome::AlreadyApplied, revision, true)
            }
            ArtworkWrite::Conflict { current } => {
                (MutationOutcome::RevisionConflict, current, false)
            }
        };
        let answer = reply(&entry.installation_id, revision, outcome);
        if commit {
            self.mutations.remember(&caller, key, hash, &answer);
        }
        Ok(Response::new(answer))
    }

    /// Decodes, shrinks and re-encodes `bytes` on a blocking worker, at most
    /// [`DECODE_SLOTS`] at a time.
    async fn prepare_image(&self, bytes: Vec<u8>, media_type: String) -> Result<Prepared, Status> {
        let _slot = Arc::clone(&self.decoders)
            .acquire_owned()
            .await
            .map_err(|_| unavailable())?;
        tokio::task::spawn_blocking(move || plugin_artwork::prepare(&bytes, &media_type))
            .await
            .map_err(|_| Status::internal("the image could not be processed"))?
            .map_err(refused)
    }
}
