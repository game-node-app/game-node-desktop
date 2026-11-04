//! Which covers to download, and how many at a time.
//!
//! A cover is wanted for every matched game of an installed entry that has no downloaded file and
//! a cover URL that passes [`cover::download_url`]. A download that failed is not tried again
//! until the next scan finishes. Downloads only run with a verified session, at most
//! [`MAX_COVER_DOWNLOADS`] at once; the rest wait here.

use std::collections::{BTreeSet, VecDeque};

use super::cover::{self, CoverUrl};
use super::machine::{Access, Effect, LibraryMachine, Phase};

/// Covers downloaded at the same time.
pub const MAX_COVER_DOWNLOADS: usize = 4;

#[derive(Debug, Default)]
pub(super) struct CoverQueue {
    waiting: VecDeque<u64>,
    running: BTreeSet<u64>,
}

impl CoverQueue {
    pub(super) fn clear(&mut self) {
        self.waiting.clear();
        self.running.clear();
    }
}

impl LibraryMachine {
    /// Queues the covers that are missing and starts as many downloads as may run now.
    /// `retry_failed` also queues covers whose last download failed.
    pub(super) fn plan_covers(&mut self, retry_failed: bool) -> Vec<Effect> {
        if self.access != Access::Verified || !matches!(self.phase, Phase::Ready) {
            return vec![];
        }
        let wanted: BTreeSet<u64> = self
            .installed_entries()
            .filter_map(|entry| self.match_status(entry).game_id())
            .filter(|game_id| {
                self.games.get(game_id).is_some_and(|game| {
                    game.cover_file.is_none()
                        && (retry_failed || game.cover_failed_at.is_none())
                        && game
                            .cover_url
                            .as_deref()
                            .and_then(cover::download_url)
                            .is_some()
                })
            })
            .collect();
        for game_id in wanted {
            if !self.covers.running.contains(&game_id) && !self.covers.waiting.contains(&game_id) {
                self.covers.waiting.push_back(game_id);
            }
        }
        self.start_cover_downloads()
    }

    fn start_cover_downloads(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        while self.covers.running.len() < MAX_COVER_DOWNLOADS {
            let Some(game_id) = self.covers.waiting.pop_front() else {
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
