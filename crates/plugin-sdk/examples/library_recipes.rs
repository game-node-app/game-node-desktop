//! Practical recipes for the library resources: read, then write with the revision you read.
//!
//! These are plain functions over [`Resources`] (from `HostClient::resources()`); copy what you
//! need. They are compiled with the SDK and run against the desktop's fixture host in its service
//! tests, so they stay true to what the host does. Run `cargo doc` for the rules each call
//! follows; the permissions document explains every status code.
#![allow(dead_code)]

use std::future::Future;
use std::time::Duration;

use gamenode_plugin_sdk::{
    Code, Cover, Outcome, Resources, Status, Written, idempotency_key, proto::LibraryEvent,
};

/// How often a change is read again and retried after another writer got in first.
const ATTEMPTS: usize = 3;
/// How often one change is resent after a lost connection or timeout.
const SENDS: usize = 3;

/// Sends one change, resending it **with the same key** if the connection fails or times out. The
/// host remembers a committed key, so an answer that was lost on the way back is returned again
/// instead of the change being applied twice.
pub async fn send<T, F, Fut>(mut call: F) -> Result<T, Status>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, Status>>,
{
    let mut last = Status::unavailable("nothing was sent");
    for attempt in 0..SENDS {
        match call().await {
            Err(status) if matches!(status.code(), Code::Unavailable | Code::DeadlineExceeded) => {
                last = status;
                tokio::time::sleep(Duration::from_millis(200 << attempt)).await;
            }
            done => return done,
        }
    }
    Err(last)
}

/// Gives an own entry a display name (`None` restores the detected one). Reads the entry for its
/// `metadata_revision` and, if it changed in the meantime, reads it again: a conflict is an
/// answer, not an error. Every attempt is a new change, so it gets a new key.
pub async fn rename(
    resources: &Resources,
    installation_id: &str,
    name: Option<&str>,
) -> Result<Written, Status> {
    for _ in 0..ATTEMPTS {
        let entry = resources.library_get(installation_id).await?;
        let key = idempotency_key();
        let written = send(|| {
            resources.set_display_name(installation_id, entry.metadata_revision, &key, name)
        })
        .await?;
        if written.outcome != Outcome::RevisionConflict {
            return Ok(written);
        }
    }
    Err(Status::aborted("the entry kept changing; try again later"))
}

/// Matches an own entry to the catalog game whose title is exactly `title` (ignoring case), if
/// the catalog has one. `Ok(None)` when nothing matches or the user already chose this entry's
/// game, which a plugin never overrides.
pub async fn match_by_title(
    resources: &Resources,
    installation_id: &str,
    title: &str,
) -> Result<Option<Written>, Status> {
    let candidates = resources.search_catalog(title, 10).await?;
    let Some(game) = candidates
        .iter()
        .find(|candidate| candidate.title.eq_ignore_ascii_case(title))
    else {
        return Ok(None);
    };
    for _ in 0..ATTEMPTS {
        let entry = resources.library_get(installation_id).await?;
        let key = idempotency_key();
        let result =
            send(|| resources.assign_match(installation_id, &game.game_id, entry.revision, &key))
                .await;
        match result {
            Ok(written) if written.outcome == Outcome::RevisionConflict => {}
            Ok(written) => return Ok(Some(written)),
            // The user's own choice stands; there is nothing for a plugin to do.
            Err(status) if status.code() == Code::FailedPrecondition => return Ok(None),
            Err(status) => return Err(status),
        }
    }
    Err(Status::aborted("the entry kept changing; try again later"))
}

/// Uses `image` (static PNG, JPEG or WebP, at most 4 MiB) as the cover of an own entry, or
/// restores the game's cover with `None`. A refused image is `INVALID_ARGUMENT` with the reason.
pub async fn use_cover(
    resources: &Resources,
    installation_id: &str,
    image: Option<(Vec<u8>, &str)>,
) -> Result<Written, Status> {
    for _ in 0..ATTEMPTS {
        let entry = resources.library_get(installation_id).await?;
        let key = idempotency_key();
        let written = send(|| async {
            match &image {
                Some((bytes, media_type)) => {
                    resources
                        .set_cover(
                            installation_id,
                            entry.artwork_revision,
                            &key,
                            bytes.clone(),
                            media_type,
                        )
                        .await
                }
                None => {
                    resources
                        .clear_cover(installation_id, entry.artwork_revision, &key)
                        .await
                }
            }
        })
        .await?;
        if written.outcome != Outcome::RevisionConflict {
            return Ok(written);
        }
    }
    Err(Status::aborted("the entry kept changing; try again later"))
}

/// The cover an own entry shows, if it has one on this device. Reading never downloads anything,
/// so `Ok(None)` just means "not yet".
pub async fn current_cover(
    resources: &Resources,
    installation_id: &str,
) -> Result<Option<Cover>, Status> {
    match resources.read_cover(installation_id, 0).await {
        Ok(cover) => Ok(Some(cover)),
        Err(status) if status.code() == Code::NotFound => Ok(None),
        Err(status) => Err(status),
    }
}

/// Stars every game one of the plugin's entries is matched to that is not starred yet, and
/// returns how many it starred. A favorite belongs to the game, so this never touches entries.
pub async fn star_matched_games(resources: &Resources) -> Result<usize, Status> {
    let starred = resources.favorite_games().await?;
    let mut changed = 0;
    for (game_id, favorite) in starred {
        if favorite {
            continue;
        }
        let key = idempotency_key();
        let written = send(|| resources.set_favorite(&game_id, true, &key)).await?;
        if written.outcome == Outcome::Applied {
            changed += 1;
        }
    }
    Ok(changed)
}

/// Keeps a view of the library current: a snapshot, then the changes after it. When the host no
/// longer retains the position (`FAILED_PRECONDITION`), list again and resume from the new
/// revision. `on_snapshot` receives the full list first and `on_event` each change after it;
/// `on_event` returns whether to keep following.
pub async fn follow_library<S, E>(
    resources: &Resources,
    mut on_snapshot: S,
    mut on_event: E,
) -> Result<(), Status>
where
    S: FnMut(&[gamenode_plugin_sdk::proto::LibraryEntry]),
    E: FnMut(LibraryEvent) -> bool,
{
    loop {
        let snapshot = resources.library_snapshot("").await?;
        on_snapshot(&snapshot.entries);
        let mut events = match resources.library_watch("", snapshot.revision).await {
            Ok(events) => events,
            Err(status) if status.code() == Code::FailedPrecondition => continue,
            Err(status) => return Err(status),
        };
        loop {
            match events.message().await {
                Ok(Some(event)) => {
                    if !on_event(event) {
                        return Ok(());
                    }
                }
                Ok(None) => return Ok(()),
                Err(status) if status.code() == Code::FailedPrecondition => break,
                Err(status) => return Err(status),
            }
        }
    }
}

fn main() {}
