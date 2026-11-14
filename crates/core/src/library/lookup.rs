//! Automatic matching: asking GameNode which game a store id is.
//!
//! A lookup is planned when the application has a verified session and a scan has just finished
//! (or the session has just become verified). It asks about every installed id that was never
//! looked up or was last looked up more than a day ago, so a scan that adds games looks up exactly
//! the new ids. The server's answer is a fact about the store id and is cached per PC. Only a
//! successful lookup writes anything: a failed one changes nothing, and the next trigger asks again.

use std::collections::{BTreeMap, BTreeSet};

use crate::game_api::{ApiError, ExternalMatch, GameSummary};

use super::machine::{Access, Effect, LibraryMachine, Phase};
use super::store::{AutoMatch, GameRecord, StoredEntry};

/// How long an answer is trusted before the id is asked about again.
pub const LOOKUP_MAX_AGE_SECS: i64 = 24 * 60 * 60;

/// A lookup that has been sent and not answered yet.
#[derive(Debug)]
pub(super) struct LookupRun {
    pub(super) op: u64,
    /// Every id that was asked about. Each gets an answer, including "no game".
    pub(super) uids: Vec<String>,
}

impl LibraryMachine {
    /// The category and uid this entry is looked up under, when its provider declares matching.
    ///
    /// A provider may name a record field as its external id; entries without that field are not
    /// looked up at all. Otherwise the store id is the uid, exactly as before.
    pub(super) fn lookup_key(&self, entry: &StoredEntry) -> Option<(u32, String)> {
        let provider = self.provider(&entry.provider)?;
        let category = provider.external_category()?;
        let uid = match provider.uid_field() {
            Some(field) => entry
                .fields
                .get(field)
                .filter(|value| !value.trim().is_empty())
                .cloned()?,
            None => entry.store_id.clone(),
        };
        Some((category, uid))
    }

    /// Starts a lookup for each category that has ids to ask about and none under way.
    pub(super) fn plan_lookups(&mut self, now: i64) -> Vec<Effect> {
        if self.access != Access::Verified || !matches!(self.phase, Phase::Ready) {
            return vec![];
        }
        // Lookups wait for the scan: it is about to change which ids exist.
        if self.scan.is_some() {
            return vec![];
        }

        let mut wanted: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
        for entry in self.visible_entries() {
            let Some((category, uid)) = self.lookup_key(entry) else {
                continue;
            };
            let fresh = self
                .auto_matches
                .get(&(category, uid.clone()))
                .is_some_and(|auto| now.saturating_sub(auto.checked_at) <= LOOKUP_MAX_AGE_SECS);
            if !fresh {
                wanted.entry(category).or_default().insert(uid);
            }
        }

        let mut effects = Vec::new();
        for (category, uids) in wanted {
            if self.lookups.contains_key(&category) {
                continue;
            }
            let uids: Vec<String> = uids.into_iter().collect();
            let op = self.next_op();
            self.lookups.insert(
                category,
                LookupRun {
                    op,
                    uids: uids.clone(),
                },
            );
            effects.push(Effect::Lookup { op, category, uids });
        }
        effects
    }

    pub(super) fn on_lookup_done(
        &mut self,
        op: u64,
        category: u32,
        result: Result<Vec<ExternalMatch>, ApiError>,
        now: i64,
    ) -> Vec<Effect> {
        if self.lookups.get(&category).is_none_or(|run| run.op != op) {
            return vec![];
        }
        let Some(run) = self.lookups.remove(&category) else {
            return vec![];
        };
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                // Nothing is written; the next scan or session change asks again.
                tracing::info!(%error, category, "the game lookup did not complete");
                return vec![];
            }
        };

        let mut by_uid: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
        for matched in &found {
            let games = by_uid.entry(matched.uid.as_str()).or_default();
            if !games.contains(&matched.game.id) {
                games.push(matched.game.id);
            }
        }
        let matches: Vec<AutoMatch> = run
            .uids
            .iter()
            .map(|uid| AutoMatch {
                category,
                uid: uid.clone(),
                game_ids: by_uid.get(uid.as_str()).cloned().unwrap_or_default(),
                checked_at: now,
            })
            .collect();
        let games = self.merge_games(found.iter().map(|matched| &matched.game));
        for auto in &matches {
            self.auto_matches
                .insert((auto.category, auto.uid.clone()), auto.clone());
        }

        // Games are stored before the matches that point at them, so an interrupted write never
        // leaves a match whose game is unknown for a whole day.
        let mut effects = vec![Effect::PersistMatches { games, matches }];
        // Ids that appeared while this lookup was out are asked about now.
        effects.extend(self.plan_lookups(now));
        effects.extend(self.plan_covers(None));
        effects
    }

    /// Applies game details to memory as `LibraryStore::save_games` applies them on disk and
    /// returns the records to store. A new cover URL drops the old downloaded file.
    pub(super) fn merge_games<'a>(
        &mut self,
        summaries: impl Iterator<Item = &'a GameSummary>,
    ) -> Vec<GameRecord> {
        let mut changed: BTreeMap<u64, GameRecord> = BTreeMap::new();
        for summary in summaries {
            let existing = self.games.get(&summary.id);
            let cover_changed = existing.is_none_or(|game| game.cover_url != summary.cover_url);
            let record = GameRecord {
                game_id: summary.id,
                // Keep a known title if the answer came without one.
                name: if summary.name.is_empty() {
                    existing.map(|game| game.name.clone()).unwrap_or_default()
                } else {
                    summary.name.clone()
                },
                release_year: summary.release_year,
                cover_url: summary.cover_url.clone(),
                cover_file: existing
                    .filter(|_| !cover_changed)
                    .and_then(|game| game.cover_file.clone()),
                cover_failed_at: existing
                    .filter(|_| !cover_changed)
                    .and_then(|game| game.cover_failed_at),
            };
            self.games.insert(record.game_id, record.clone());
            changed.insert(record.game_id, record);
        }
        changed.into_values().collect()
    }
}

#[cfg(test)]
mod tests;
