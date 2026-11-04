//! Playtime repository sharing the settings connection and FIFO storage worker.

use gamenode_core::playtime::{Session, Total};
use gamenode_core::ports::{PlaytimeStore, StorageError};
use gamenode_core::sync::SyncPreferences;
use rusqlite::{Connection, OptionalExtension, params};
use std::sync::{Arc, Mutex};

pub const KEY_PLAYTIME_SOURCE: &str = "playtime_source";
pub const KEY_SYNC_ENABLED: &str = "sync_enabled";
pub const KEY_SYNC_EXPLAINED: &str = "sync_explained";

pub struct SqlitePlaytimeStore {
    conn: Arc<Mutex<Connection>>,
}
fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}
fn db_id(id: Option<u64>) -> Result<Option<i64>, StorageError> {
    id.map(i64::try_from).transpose().map_err(failed)
}
impl SqlitePlaytimeStore {
    pub(crate) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}
impl PlaytimeStore for SqlitePlaytimeStore {
    fn load_source(&self) -> Result<gamenode_core::playtime::PlaytimeSource, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let value = conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [KEY_PLAYTIME_SOURCE],
                |r| r.get::<_, String>(0),
            )
            .optional()
            .map_err(failed)?;
        Ok(gamenode_core::playtime::PlaytimeSource::parse(
            value.as_deref(),
        ))
    }
    fn save_source(
        &self,
        source: gamenode_core::playtime::PlaytimeSource,
    ) -> Result<(), StorageError> {
        crate::db::lock(&self.conn).execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![KEY_PLAYTIME_SOURCE,source.as_str()]).map(drop).map_err(failed)
    }

    fn open_session(&self, s: &Session) -> Result<(), StorageError> {
        if s.ended_at.is_some() {
            return Err(StorageError::Failed("cannot open a closed session".into()));
        }
        crate::db::lock(&self.conn).execute(
            "INSERT INTO playtime_sessions (id,account_id,entry_key,game_id,source,platform_id,started_at,checkpoint_at,ended_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL)",
            params![s.id,s.account_id,s.entry_key,db_id(s.game_id)?,s.source,s.platform_id,s.started_at,s.checkpoint_at],
        ).map(drop).map_err(failed)
    }
    fn checkpoint(&self, account: &str, id: &str, at: i64) -> Result<(), StorageError> {
        crate::db::lock(&self.conn).execute("UPDATE playtime_sessions SET checkpoint_at = MAX(checkpoint_at,?3) WHERE account_id=?1 AND id=?2 AND ended_at IS NULL", params![account,id,at]).map(drop).map_err(failed)
    }
    fn close_session(
        &self,
        account: &str,
        id: &str,
        at: i64,
        game_id: Option<u64>,
    ) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute("UPDATE playtime_sessions SET ended_at=?3, game_id=?4 WHERE account_id=?1 AND id=?2 AND ended_at IS NULL", params![account,id,at,db_id(game_id)?]).map_err(failed)?;
        let dropped = tx.execute("DELETE FROM playtime_sessions WHERE account_id=?1 AND id=?2 AND ended_at <= started_at", params![account,id]).map_err(failed)?;
        if dropped > 0 {
            tracing::warn!("dropped playtime session with non-positive duration");
        }
        tx.commit().map_err(failed)
    }
    fn recover(&self) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "UPDATE playtime_sessions SET ended_at=checkpoint_at WHERE ended_at IS NULL",
            [],
        )
        .map_err(failed)?;
        let dropped = tx
            .execute(
                "DELETE FROM playtime_sessions WHERE ended_at <= started_at",
                [],
            )
            .map_err(failed)?;
        if dropped > 0 {
            tracing::warn!(
                dropped,
                "dropped playtime sessions with non-positive duration"
            );
        }
        tx.commit().map_err(failed)
    }
    fn sessions(&self, account: &str) -> Result<Vec<Session>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let mut stmt = conn.prepare("SELECT id,account_id,entry_key,game_id,source,platform_id,started_at,checkpoint_at,ended_at FROM playtime_sessions WHERE account_id=?1 ORDER BY started_at,id").map_err(failed)?;
        stmt.query_map([account], |r| {
            Ok(Session {
                id: r.get(0)?,
                account_id: r.get(1)?,
                entry_key: r.get(2)?,
                game_id: r
                    .get::<_, Option<i64>>(3)?
                    .and_then(|id| u64::try_from(id).ok()),
                source: r.get(4)?,
                platform_id: r.get(5)?,
                started_at: r.get(6)?,
                checkpoint_at: r.get(7)?,
                ended_at: r.get(8)?,
            })
        })
        .map_err(failed)?
        .collect::<rusqlite::Result<_>>()
        .map_err(failed)
    }
    fn totals(&self, account: &str) -> Result<Vec<Total>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let mut stmt = conn.prepare("SELECT account_id,entry_key,total_seconds,last_played_at,accepted_game_id,accepted_seconds,rejected_seconds FROM playtime_totals WHERE account_id=?1 ORDER BY entry_key").map_err(failed)?;
        stmt.query_map([account], |r| {
            Ok(Total {
                account_id: r.get(0)?,
                entry_key: r.get(1)?,
                total_seconds: r.get(2)?,
                last_played_at: r.get(3)?,
                accepted_game_id: r
                    .get::<_, Option<i64>>(4)?
                    .and_then(|id| u64::try_from(id).ok()),
                accepted_seconds: r.get(5)?,
                rejected_seconds: r.get(6)?,
            })
        })
        .map_err(failed)?
        .collect::<rusqlite::Result<_>>()
        .map_err(failed)
    }
    fn save_total(&self, t: &Total) -> Result<(), StorageError> {
        crate::db::lock(&self.conn).execute(
            "INSERT INTO playtime_totals (account_id,entry_key,total_seconds,last_played_at,accepted_game_id,accepted_seconds,rejected_seconds) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(account_id,entry_key) DO UPDATE SET total_seconds=excluded.total_seconds,last_played_at=excluded.last_played_at,accepted_game_id=excluded.accepted_game_id,accepted_seconds=excluded.accepted_seconds,rejected_seconds=excluded.rejected_seconds",
            params![t.account_id,t.entry_key,t.total_seconds,t.last_played_at,db_id(t.accepted_game_id)?,t.accepted_seconds,t.rejected_seconds],
        ).map(drop).map_err(failed)
    }
    fn load_sync(&self) -> Result<SyncPreferences, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let read = |key| {
            conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get::<_, String>(0)
            })
            .optional()
            .map_err(failed)
        };
        Ok(SyncPreferences::parse(
            read(KEY_SYNC_ENABLED)?.as_deref(),
            read(KEY_SYNC_EXPLAINED)?.as_deref(),
        ))
    }
    fn save_sync(&self, preferences: SyncPreferences) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        for (key, value) in [
            (KEY_SYNC_ENABLED, preferences.enabled),
            (KEY_SYNC_EXPLAINED, preferences.explained),
        ] {
            tx.execute("INSERT INTO settings (key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key,if value { "1" } else { "0" }]).map_err(failed)?;
        }
        tx.commit().map_err(failed)
    }
}
#[cfg(test)]
mod tests;
