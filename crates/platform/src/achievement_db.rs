//! SQLite achievement cache keyed by provider and store account.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use gamenode_core::achievements::{
    AchievementGameState, AchievementRecord, AchievementStore, AchievementUnlock,
    StoredAchievementGame, StoredAchievementSummary,
};
use gamenode_core::library::CatalogError;
use gamenode_core::ports::StorageError;
use rusqlite::{Connection, OptionalExtension, params};

fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}

fn parse_failure(value: Option<String>) -> Option<CatalogError> {
    value.and_then(|value| CatalogError::from_category(&value))
}

pub struct SqliteAchievementStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteAchievementStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

impl AchievementStore for SqliteAchievementStore {
    fn load_summaries(
        &self,
        provider: &str,
        account_id: &str,
    ) -> Result<Vec<StoredAchievementSummary>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let mut latest = BTreeMap::new();
        {
            let mut statement = conn
                .prepare(
                    "SELECT entry_id, achievement_id, name, unlocked_at FROM (
                        SELECT entry_id, achievement_id, name, unlocked_at,
                               ROW_NUMBER() OVER (
                                   PARTITION BY entry_id
                                   ORDER BY (unlocked_at IS NULL), unlocked_at DESC, position
                               ) AS row_number
                        FROM achievements
                        WHERE provider = ?1 AND store_account_id = ?2 AND unlocked = 1
                     ) WHERE row_number = 1",
                )
                .map_err(failed)?;
            let rows = statement
                .query_map(params![provider, account_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        AchievementUnlock {
                            id: row.get(1)?,
                            name: row.get(2)?,
                            unlocked_at: row.get(3)?,
                        },
                    ))
                })
                .map_err(failed)?;
            for row in rows {
                let (entry_id, unlock) = row.map_err(failed)?;
                latest.insert(entry_id, unlock);
            }
        }
        let mut statement = conn
            .prepare(
                "SELECT g.entry_id, g.state, g.language, g.last_success_at, g.last_attempt_at,
                        g.failure,
                        (SELECT COUNT(*) FROM achievements a
                          WHERE a.provider = g.provider
                            AND a.store_account_id = g.store_account_id
                            AND a.entry_id = g.entry_id),
                        (SELECT COUNT(*) FROM achievements a
                          WHERE a.provider = g.provider
                            AND a.store_account_id = g.store_account_id
                            AND a.entry_id = g.entry_id AND a.unlocked = 1)
                   FROM achievement_games g
                  WHERE g.provider = ?1 AND g.store_account_id = ?2
                  ORDER BY g.entry_id",
            )
            .map_err(failed)?;
        let rows = statement
            .query_map(params![provider, account_id], |row| {
                let entry_id: String = row.get(0)?;
                let state: String = row.get(1)?;
                Ok((
                    entry_id.clone(),
                    StoredAchievementSummary {
                        entry_id,
                        state: AchievementGameState::parse(&state)
                            .unwrap_or(AchievementGameState::Failed),
                        language: row.get(2)?,
                        last_success_at: row.get(3)?,
                        last_attempt_at: row.get(4)?,
                        failure: parse_failure(row.get(5)?),
                        total: usize::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
                        unlocked: usize::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
                        latest_unlock: None,
                    },
                ))
            })
            .map_err(failed)?;
        let mut summaries = Vec::new();
        for row in rows {
            let (entry_id, mut summary) = row.map_err(failed)?;
            summary.latest_unlock = latest.remove(&entry_id);
            summaries.push(summary);
        }
        Ok(summaries)
    }

    fn load_game(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
    ) -> Result<Option<StoredAchievementGame>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let row = conn
            .query_row(
                "SELECT state, language, last_success_at, last_attempt_at, failure
                   FROM achievement_games
                  WHERE provider = ?1 AND store_account_id = ?2 AND entry_id = ?3",
                params![provider, account_id, entry_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()
            .map_err(failed)?;
        let Some((state, language, last_success_at, last_attempt_at, failure)) = row else {
            return Ok(None);
        };
        let mut statement = conn
            .prepare(
                "SELECT achievement_id, name, description, hidden, icon_url, icon_gray_url,
                        unlocked, unlocked_at, position
                   FROM achievements
                  WHERE provider = ?1 AND store_account_id = ?2 AND entry_id = ?3
                  ORDER BY position",
            )
            .map_err(failed)?;
        let achievements = statement
            .query_map(params![provider, account_id, entry_id], |row| {
                Ok(AchievementRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    hidden: row.get::<_, i64>(3)? != 0,
                    icon_url: row.get(4)?,
                    icon_gray_url: row.get(5)?,
                    unlocked: row.get::<_, i64>(6)? != 0,
                    unlocked_at: row.get(7)?,
                    position: row.get(8)?,
                })
            })
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?;
        Ok(Some(StoredAchievementGame {
            entry_id: entry_id.to_owned(),
            state: AchievementGameState::parse(&state).unwrap_or(AchievementGameState::Failed),
            language,
            last_success_at,
            last_attempt_at,
            failure: parse_failure(failure),
            achievements,
        }))
    }

    fn save_ready(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
        language: &str,
        achievements: &[AchievementRecord],
        now: i64,
    ) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "INSERT INTO achievement_games
                (provider, store_account_id, entry_id, state, language, last_success_at,
                 last_attempt_at, failure)
             VALUES (?1, ?2, ?3, 'ready', ?4, ?5, ?5, NULL)
             ON CONFLICT (provider, store_account_id, entry_id) DO UPDATE SET
                state = 'ready', language = excluded.language,
                last_success_at = excluded.last_success_at,
                last_attempt_at = excluded.last_attempt_at, failure = NULL",
            params![provider, account_id, entry_id, language, now],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM achievements
              WHERE provider = ?1 AND store_account_id = ?2 AND entry_id = ?3",
            params![provider, account_id, entry_id],
        )
        .map_err(failed)?;
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO achievements
                        (provider, store_account_id, entry_id, achievement_id, name, description,
                         hidden, icon_url, icon_gray_url, unlocked, unlocked_at, position)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                )
                .map_err(failed)?;
            for (position, achievement) in achievements.iter().enumerate() {
                insert
                    .execute(params![
                        provider,
                        account_id,
                        entry_id,
                        achievement.id,
                        achievement.name,
                        achievement.description,
                        i64::from(achievement.hidden),
                        achievement.icon_url,
                        achievement.icon_gray_url,
                        i64::from(achievement.unlocked),
                        achievement.unlocked_at,
                        i64::try_from(position).unwrap_or(i64::MAX),
                    ])
                    .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    fn save_none(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
        language: &str,
        now: i64,
    ) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "INSERT INTO achievement_games
                (provider, store_account_id, entry_id, state, language, last_success_at,
                 last_attempt_at, failure)
             VALUES (?1, ?2, ?3, 'none', ?4, ?5, ?5, NULL)
             ON CONFLICT (provider, store_account_id, entry_id) DO UPDATE SET
                state = 'none', language = excluded.language,
                last_success_at = excluded.last_success_at,
                last_attempt_at = excluded.last_attempt_at, failure = NULL",
            params![provider, account_id, entry_id, language, now],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM achievements
              WHERE provider = ?1 AND store_account_id = ?2 AND entry_id = ?3",
            params![provider, account_id, entry_id],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)
    }

    fn save_failure(
        &self,
        provider: &str,
        account_id: &str,
        entry_id: &str,
        failure: CatalogError,
        now: i64,
    ) -> Result<(), StorageError> {
        let conn = crate::db::lock(&self.conn);
        conn.execute(
            "INSERT INTO achievement_games
                (provider, store_account_id, entry_id, state, language, last_success_at,
                 last_attempt_at, failure)
             VALUES (?1, ?2, ?3, 'failed', '', NULL, ?4, ?5)
             ON CONFLICT (provider, store_account_id, entry_id) DO UPDATE SET
                last_attempt_at = excluded.last_attempt_at,
                failure = excluded.failure",
            params![provider, account_id, entry_id, now, failure.as_str()],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn delete_account(&self, provider: &str, account_id: &str) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "DELETE FROM achievements WHERE provider = ?1 AND store_account_id = ?2",
            params![provider, account_id],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM achievement_games WHERE provider = ?1 AND store_account_id = ?2",
            params![provider, account_id],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{MIGRATIONS, open_with};

    fn store() -> (tempfile::TempDir, SqliteAchievementStore) {
        let temp = tempfile::tempdir().unwrap();
        let conn = open_with(&temp.path().join("profile.db"), MIGRATIONS).unwrap();
        (
            temp,
            SqliteAchievementStore::new(Arc::new(Mutex::new(conn))),
        )
    }

    fn achievement(id: &str, unlocked: bool, at: Option<i64>, position: i64) -> AchievementRecord {
        AchievementRecord {
            id: id.into(),
            name: format!("Name {id}"),
            description: format!("Description {id}"),
            hidden: id == "secret",
            icon_url: format!("https://cdn.example/{id}.jpg"),
            icon_gray_url: format!("https://cdn.example/{id}_gray.jpg"),
            unlocked,
            unlocked_at: at,
            position,
        }
    }

    #[test]
    fn ready_replaces_one_game_atomically_and_removed_definitions_disappear() {
        let (_temp, store) = store();
        store
            .save_ready(
                "steam",
                "A",
                "620",
                "english",
                &[
                    achievement("one", true, Some(10), 0),
                    achievement("two", false, None, 1),
                ],
                100,
            )
            .unwrap();
        store
            .save_ready(
                "steam",
                "A",
                "620",
                "portuguese",
                &[achievement("one", true, Some(10), 0)],
                200,
            )
            .unwrap();
        let game = store.load_game("steam", "A", "620").unwrap().unwrap();
        assert_eq!(game.state, AchievementGameState::Ready);
        assert_eq!(game.language, "portuguese");
        assert_eq!(game.last_success_at, Some(200));
        assert_eq!(game.achievements.len(), 1);
        assert_eq!(game.achievements[0].id, "one");
        let summary = store.load_summaries("steam", "A").unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].unlocked, 1);
        assert_eq!(summary[0].total, 1);
        assert_eq!(summary[0].latest_unlock.as_ref().unwrap().id, "one");
    }

    #[test]
    fn a_failure_preserves_previous_data_and_marks_the_attempt() {
        let (_temp, store) = store();
        store
            .save_ready(
                "steam",
                "A",
                "620",
                "english",
                &[achievement("one", true, Some(10), 0)],
                100,
            )
            .unwrap();
        store
            .save_failure("steam", "A", "620", CatalogError::Privacy, 200)
            .unwrap();
        let game = store.load_game("steam", "A", "620").unwrap().unwrap();
        assert_eq!(game.state, AchievementGameState::Ready);
        assert_eq!(game.last_success_at, Some(100));
        assert_eq!(game.last_attempt_at, 200);
        assert_eq!(game.failure, Some(CatalogError::Privacy));
        assert_eq!(game.achievements.len(), 1);
        let summary = store.load_summaries("steam", "A").unwrap();
        assert_eq!(summary[0].failure, Some(CatalogError::Privacy));

        store
            .save_failure("steam", "A", "70", CatalogError::Unreachable, 300)
            .unwrap();
        let never = store.load_game("steam", "A", "70").unwrap().unwrap();
        assert_eq!(never.state, AchievementGameState::Failed);
        assert!(never.achievements.is_empty());
    }

    #[test]
    fn none_clears_definitions_and_reads_are_account_scoped() {
        let (_temp, store) = store();
        store
            .save_ready(
                "steam",
                "A",
                "620",
                "english",
                &[achievement("one", true, Some(10), 0)],
                100,
            )
            .unwrap();
        store
            .save_none("steam", "A", "620", "english", 200)
            .unwrap();
        let game = store.load_game("steam", "A", "620").unwrap().unwrap();
        assert_eq!(game.state, AchievementGameState::None);
        assert!(game.achievements.is_empty());
        assert!(store.load_game("steam", "B", "620").unwrap().is_none());
        assert!(store.load_summaries("steam", "B").unwrap().is_empty());

        store
            .save_ready(
                "steam",
                "B",
                "620",
                "english",
                &[achievement("one", false, None, 0)],
                300,
            )
            .unwrap();
        assert_eq!(store.load_summaries("steam", "A").unwrap().len(), 1);
        assert_eq!(store.load_summaries("steam", "B").unwrap().len(), 1);
    }

    #[test]
    fn deleting_one_account_keeps_another_account_and_provider() {
        let (_temp, store) = store();
        for (account, provider) in [("A", "steam"), ("B", "steam"), ("A", "other")] {
            store
                .save_ready(
                    provider,
                    account,
                    "620",
                    "english",
                    &[achievement("one", true, Some(10), 0)],
                    100,
                )
                .unwrap();
        }
        store.delete_account("steam", "A").unwrap();
        assert!(store.load_game("steam", "A", "620").unwrap().is_none());
        assert!(store.load_game("steam", "B", "620").unwrap().is_some());
        assert!(store.load_game("other", "A", "620").unwrap().is_some());
    }
}
