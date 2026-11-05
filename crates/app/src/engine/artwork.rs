//! Persistent landscape images, kept separately from the portrait cover cache.

use std::path::Path;

use gamenode_core::SessionCredential;
use gamenode_core::game_api::{ApiError, GameApi, MAX_COVER_BYTES};
use gamenode_platform::covers::{cover_file_name, write_cover};

pub(super) async fn load(
    api: &dyn GameApi,
    credential: Option<&SessionCredential>,
    dir: &Path,
    game_id: u64,
) -> Result<Option<Vec<u8>>, ApiError> {
    let path = dir.join(cover_file_name(game_id));
    let cached = tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > MAX_COVER_BYTES as u64 {
            return None;
        }
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(file, MAX_COVER_BYTES as u64 + 1),
            &mut bytes,
        )
        .ok()?;
        (bytes.len() <= MAX_COVER_BYTES && bytes.starts_with(&[0xff, 0xd8, 0xff])).then_some(bytes)
    })
    .await
    .ok()
    .flatten();
    if let Some(bytes) = cached {
        return Ok(Some(bytes));
    }
    let credential = credential.ok_or(ApiError::Failed)?;
    let result = api.fetch_artwork(credential, game_id).await?;
    if let Some(bytes) = &result {
        let (dir, bytes) = (dir.to_owned(), bytes.clone());
        // A cache write failure must not prevent displaying a downloaded image.
        let stored = tokio::task::spawn_blocking(move || write_cover(&dir, game_id, &bytes)).await;
        if !matches!(stored, Ok(Ok(_))) {
            tracing::warn!(game_id, "could not cache game artwork");
        }
    }
    Ok(result)
}
