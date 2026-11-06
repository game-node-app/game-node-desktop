//! Which covers to download, and how many at a time.
//!
//! A cover is wanted for every matched game of an installed entry that has no downloaded file and
//! a cover URL that passes [`cover::download_url`]. A download that failed is not tried again
//! until the next scan finishes, and then only once [`COVER_RETRY_AFTER_SECS`] have passed since
//! it failed, so a flaky network does not download the same covers on every scan. Downloads only run with a verified session, at most
//! [`MAX_COVER_DOWNLOADS`] at once; the rest wait here.

use std::collections::{BTreeSet, VecDeque};

use super::cover::{self, CoverUrl};
use super::machine::{Access, Effect, LibraryMachine, Phase};

/// Covers downloaded at the same time.
pub const MAX_COVER_DOWNLOADS: usize = 4;

/// How long a failed cover download waits before a scan may try it again.
pub const COVER_RETRY_AFTER_SECS: i64 = 5 * 60;

#[derive(Debug, Default)]
pub(super) struct CoverQueue {
    waiting: VecDeque<u64>,
    /// The ids in `waiting`, so queuing a cover is not a linear search.
    queued: BTreeSet<u64>,
    running: BTreeSet<u64>,
}

impl CoverQueue {
    pub(super) fn clear(&mut self) {
        self.waiting.clear();
        self.queued.clear();
        self.running.clear();
    }

    fn push(&mut self, game_id: u64) {
        if !self.running.contains(&game_id) && self.queued.insert(game_id) {
            self.waiting.push_back(game_id);
        }
    }

    fn pop(&mut self) -> Option<u64> {
        let game_id = self.waiting.pop_front()?;
        self.queued.remove(&game_id);
        Some(game_id)
    }
}

impl LibraryMachine {
    /// Queues the covers that are missing and starts as many downloads as may run now.
    /// `retry_failed_at` is the current time when covers whose last download failed at least
    /// [`COVER_RETRY_AFTER_SECS`] ago should be queued again, and `None` to leave them alone.
    pub(super) fn plan_covers(&mut self, retry_failed_at: Option<i64>) -> Vec<Effect> {
        if self.access != Access::Verified || !matches!(self.phase, Phase::Ready) {
            return vec![];
        }
        let wanted: BTreeSet<u64> = self
            .installed_entries()
            .filter_map(|entry| self.match_status(entry).game_id())
            .filter(|game_id| {
                self.games.get(game_id).is_some_and(|game| {
                    game.cover_file.is_none()
                        && game.cover_failed_at.is_none_or(|failed_at| {
                            retry_failed_at.is_some_and(|now| {
                                now.saturating_sub(failed_at) >= COVER_RETRY_AFTER_SECS
                            })
                        })
                        && game
                            .cover_url
                            .as_deref()
                            .and_then(cover::download_url)
                            .is_some()
                })
            })
            .collect();
        for game_id in wanted {
            self.covers.push(game_id);
        }
        self.start_cover_downloads()
    }

    fn start_cover_downloads(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        while self.covers.running.len() < MAX_COVER_DOWNLOADS {
            let Some(game_id) = self.covers.pop() else {
                break;
            };
            let url: Option<CoverUrl> = self
                .games
                .get(&game_id)
                .and_then(|game| game.cover_url.as_deref())
                .and_then(cover::download_url);
            let Some(url) = url else {
                continue;
            };
            self.covers.running.insert(game_id);
            effects.push(Effect::FetchCover { game_id, url });
        }
        effects
    }

    /// The outcome of an [`Effect::FetchCover`]: the stored file name, or that it failed.
    pub(super) fn on_cover_done(
        &mut self,
        game_id: u64,
        result: Result<String, ()>,
        now: i64,
    ) -> Vec<Effect> {
        if !self.covers.running.remove(&game_id) {
            return vec![];
        }
        let mut effects = Vec::new();
        if let Some(game) = self.games.get_mut(&game_id) {
            match result {
                Ok(file) => {
                    game.cover_file = Some(file.clone());
                    game.cover_failed_at = None;
                    effects.push(Effect::PersistCoverReady { game_id, file });
                }
                Err(()) => {
                    game.cover_failed_at = Some(now);
                    effects.push(Effect::PersistCoverFailed { game_id, at: now });
                }
            }
        }
        effects.extend(self.start_cover_downloads());
        effects
    }
}

#[cfg(test)]
mod tests;
