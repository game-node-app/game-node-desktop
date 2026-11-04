use super::*;
use crate::db::{MIGRATIONS, SqliteSettingsStore};
use gamenode_core::ports::SettingsStore;
use tempfile::TempDir;
fn session(account: &str, id: &str) -> Session {
    Session {
        id: id.into(),
        account_id: account.into(),
        entry_key: "steam:620".into(),
        game_id: None,
        source: "steam".into(),
        platform_id: 6,
        started_at: 100,
        checkpoint_at: 100,
        ended_at: None,
    }
}
fn total(account: &str) -> Total {
    Total {
        account_id: account.into(),
        entry_key: "steam:620".into(),
        total_seconds: 45240,
        last_played_at: Some(100),
        accepted_game_id: Some(620),
        accepted_seconds: Some(45000),
        rejected_seconds: None,
    }
}
#[test]
fn crash_recovery_preserves_ids_times_and_accounts_across_restart() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("db");
    {
        let db = SqliteSettingsStore::open(&path).unwrap();
        let store = db.playtime_store();
        store.open_session(&session("a", "first")).unwrap();
        store.open_session(&session("b", "second")).unwrap();
        store.checkpoint("a", "first", 160).unwrap();
        store.checkpoint("b", "second", 180).unwrap();
        store.close_session("b", "first", 190, Some(9)).unwrap(); // another account cannot close it
        assert_eq!(store.sessions("a").unwrap()[0].ended_at, None);
        store.save_total(&total("a")).unwrap();
        let mut other = total("b");
        other.total_seconds = 1;
        store.save_total(&other).unwrap();
    }
    let db = SqliteSettingsStore::open(&path).unwrap();
    let store = db.playtime_store();
    store.recover().unwrap();
    store.recover().unwrap();
    let first = &store.sessions("a").unwrap()[0];
    assert_eq!(
        (
            first.id.as_str(),
            first.started_at,
            first.checkpoint_at,
            first.ended_at
        ),
        ("first", 100, 160, Some(160))
    );
    assert_eq!(store.sessions("b").unwrap()[0].ended_at, Some(180));
    assert_eq!(store.totals("a").unwrap(), [total("a")]);
    assert_eq!(store.totals("b").unwrap()[0].total_seconds, 1);
    assert!(store.sessions("c").unwrap().is_empty());
}
#[test]
fn close_uses_match_at_close_and_drops_nonpositive_sessions() {
    let db = SqliteSettingsStore::new(crate::db::open(std::path::Path::new(":memory:")).unwrap());
    let store = db.playtime_store();
    store.open_session(&session("a", "valid")).unwrap();
    store.close_session("a", "valid", 200, Some(620)).unwrap();
    store.checkpoint("a", "valid", 300).unwrap();
    assert_eq!(store.sessions("a").unwrap()[0].game_id, Some(620));
    assert_eq!(store.sessions("a").unwrap()[0].ended_at, Some(200));
    for (id, end) in [("zero", 100), ("negative", 90)] {
        store.open_session(&session("a", id)).unwrap();
        store.close_session("a", id, end, None).unwrap();
    }
    store.open_session(&session("a", "no-checkpoint")).unwrap();
    store.recover().unwrap();
    assert_eq!(store.sessions("a").unwrap().len(), 1);
}
#[test]
fn preferences_default_persist_and_fail_closed_on_corruption() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("db");
    let db = SqliteSettingsStore::open(&path).unwrap();
    assert_eq!(
        db.playtime_store().load_sync().unwrap(),
        SyncPreferences::default()
    );
    db.playtime_store()
        .save_sync(SyncPreferences {
            enabled: false,
            explained: true,
        })
        .unwrap();
    drop(db);
    let db = SqliteSettingsStore::open(&path).unwrap();
    assert_eq!(
        db.playtime_store().load_sync().unwrap(),
        SyncPreferences {
            enabled: false,
            explained: true
        }
    );
    db.conn()
        .execute(
            "UPDATE settings SET value='broken' WHERE key='sync_enabled'",
            [],
        )
        .unwrap();
    assert_eq!(
        db.playtime_store().load_sync().unwrap(),
        SyncPreferences {
            enabled: false,
            explained: false
        }
    );
}
#[test]
fn step_two_upgrade_preserves_settings_entries_and_matches() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("db");
    let conn = crate::db::open_with(&path, &MIGRATIONS[..2]).unwrap();
    conn.execute(
        "UPDATE settings SET value='software' WHERE key='renderer'",
        [],
    )
    .unwrap();
    conn.execute("INSERT INTO library_entries (key,provider,store_id,name,install_dir,fields_json,installed,first_seen,last_seen) VALUES ('steam:620','steam','620','Portal 2','C:/Games/Portal','{}',1,100,100)",[]).unwrap();
    conn.execute(
        "INSERT INTO chosen_matches (account_id,entry_key,game_id,chosen_at) VALUES ('a','steam:620',620,100)",
        [],
    )
    .unwrap();
    drop(conn);
    let db = SqliteSettingsStore::open(&path).unwrap();
    assert_eq!(
        db.load().unwrap().renderer,
        gamenode_core::RendererPreference::Software
    );
    let conn = db.conn();
    assert_eq!(
        conn.query_row("SELECT name FROM library_entries", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "Portal 2"
    );
    assert_eq!(
        conn.query_row("SELECT game_id FROM chosen_matches", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        620
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM playtime_sessions", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn playtime_source_defaults_persists_and_recovers_from_invalid_data() {
    use gamenode_core::playtime::PlaytimeSource;
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("db");
    let db = SqliteSettingsStore::open(&path).unwrap();
    assert_eq!(
        db.playtime_store().load_source().unwrap(),
        PlaytimeSource::Store
    );
    db.playtime_store()
        .save_source(PlaytimeSource::Internal)
        .unwrap();
    drop(db);
    let db = SqliteSettingsStore::open(&path).unwrap();
    assert_eq!(
        db.playtime_store().load_source().unwrap(),
        PlaytimeSource::Internal
    );
    db.conn()
        .execute(
            "UPDATE settings SET value='bad' WHERE key='playtime_source'",
            [],
        )
        .unwrap();
    assert_eq!(
        db.playtime_store().load_source().unwrap(),
        PlaytimeSource::Store
    );
}
