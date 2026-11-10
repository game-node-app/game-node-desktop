//! Bounded staging for account catalog categories. Callers commit only after
//! finish succeeds; accepting a chunk never changes persisted library facts.

use std::collections::BTreeSet;

use crate::{WireError, models::*, proto::*};

pub const MAX_RECORDS: usize = 50_000;
pub const CHUNK_RECORDS: usize = 256;
pub const MAX_TRANSFER_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
struct Category {
    next_index: u32,
    games: Vec<CatalogRecord>,
    ids: BTreeSet<String>,
}

#[derive(Default)]
pub struct CatalogTransfer {
    owned: Category,
    family: Category,
    bytes: usize,
}

impl CatalogTransfer {
    pub fn push(&mut self, chunk: CatalogChunk) -> Result<(), WireError> {
        let buffer = match CatalogCategory::try_from(chunk.category).map_err(|_| WireError)? {
            CatalogCategory::Owned => &mut self.owned,
            CatalogCategory::Family => &mut self.family,
            _ => return Err(WireError),
        };
        if chunk.index != buffer.next_index
            || chunk.games.is_empty()
            || chunk.games.len() > CHUNK_RECORDS
            || buffer.games.len() + chunk.games.len() > MAX_RECORDS
        {
            return Err(WireError);
        }
        let games = chunk
            .games
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<CatalogRecord>, _>>()?;
        let bytes = games.iter().try_fold(self.bytes, |size, game| {
            let fields = game
                .fields
                .iter()
                .try_fold(0usize, |total, (key, value)| {
                    total.checked_add(key.len())?.checked_add(value.len())
                })
                .ok_or(WireError)?;
            size.checked_add(game.id.len() + game.name.len() + fields)
                .ok_or(WireError)
        })?;
        if bytes > MAX_TRANSFER_BYTES {
            return Err(WireError);
        }
        let mut chunk_ids = BTreeSet::new();
        if games
            .iter()
            .any(|game| buffer.ids.contains(&game.id) || !chunk_ids.insert(game.id.clone()))
        {
            return Err(WireError);
        }
        buffer.next_index = buffer.next_index.checked_add(1).ok_or(WireError)?;
        buffer.ids.extend(chunk_ids);
        buffer.games.extend(games);
        self.bytes = bytes;
        Ok(())
    }

    pub fn finish(
        self,
        completions: Vec<CatalogCompletion>,
        now: i64,
    ) -> Result<CatalogUpdate, WireError> {
        if now < 0 || completions.len() > 2 {
            return Err(WireError);
        }
        let mut owned = Some(self.owned);
        let mut family = Some(self.family);
        let mut update = CatalogUpdate {
            now,
            ..Default::default()
        };
        for completion in completions {
            let category = CatalogCategory::try_from(completion.category).map_err(|_| WireError)?;
            let buffer = match category {
                CatalogCategory::Owned => owned.take(),
                CatalogCategory::Family => family.take(),
                _ => return Err(WireError),
            }
            .ok_or(WireError)?;
            let outcome = match completion.outcome.ok_or(WireError)? {
                catalog_completion::Outcome::CompletedRecords(count)
                    if count as usize == buffer.games.len() =>
                {
                    Some(Ok(buffer.games))
                }
                catalog_completion::Outcome::Failed(code) => {
                    Some(Err(CatalogFailure::try_from(code)
                        .map_err(|_| WireError)?
                        .try_into()?))
                }
                catalog_completion::Outcome::Unchanged(true) => None,
                _ => return Err(WireError),
            };
            match category {
                CatalogCategory::Owned => update.owned = outcome,
                CatalogCategory::Family => update.family = outcome,
                _ => return Err(WireError),
            }
        }
        // A transmitted category cannot disappear from the terminal result.
        if owned.is_some_and(|buffer| !buffer.games.is_empty())
            || family.is_some_and(|buffer| !buffer.games.is_empty())
        {
            return Err(WireError);
        }
        Ok(update)
    }
}
