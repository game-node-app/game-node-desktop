//! Worker-only durable DRM-free registrations, discovery and save jobs.
use gamenode_core::drm_free::*;
use gamenode_core::ports::StorageError;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use std::sync::{Arc, Mutex};

pub struct SqliteLocalGameStore {
    conn: Arc<Mutex<Connection>>,
}
fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}
fn encode(value: &impl Serialize) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(failed)
}
// Upgrade path spelling at the storage boundary without changing identity,
// revisions, candidate fingerprints, or requiring the folder to be online.
fn registration_paths(mut value: Registration) -> Registration {
    for path in [
        &mut value.executable,
        &mut value.root,
        &mut value.working_directory,
    ]
    .into_iter()
    .chain(value.gameplay.iter_mut())
    .chain(value.launchers.iter_mut())
    {
        *path = crate::host::strip_verbatim_prefix(std::mem::take(path));
    }
    value
}
fn watch_paths(mut value: WatchRoot) -> WatchRoot {
    value.path = crate::host::strip_verbatim_prefix(value.path);
    value
}
fn candidate_paths(mut value: Candidate) -> Candidate {
    for path in std::iter::once(&mut value.root)
        .chain(value.targets.iter_mut())
        .chain(value.selected_target.iter_mut())
    {
        *path = crate::host::strip_verbatim_prefix(std::mem::take(path));
    }
    value
}
impl SqliteLocalGameStore {
    pub(crate) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
    fn list<T: DeserializeOwned>(&self, sql: &str) -> Result<Vec<T>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let mut statement = conn.prepare(sql).map_err(failed)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        rows.map(|row| serde_json::from_str(&row.map_err(failed)?).map_err(failed))
            .collect()
    }
}
impl LocalGameStore for SqliteLocalGameStore {
    fn registrations(&self) -> Result<Vec<Registration>, StorageError> {
        self.list("SELECT payload FROM manual_registrations ORDER BY id")
            .map(|rows| rows.into_iter().map(registration_paths).collect())
    }
    fn save_registration(&self, v: &Registration) -> Result<(), StorageError> {
        let v = &registration_paths(v.clone());
        crate::db::lock(&self.conn).execute("INSERT INTO manual_registrations(id,file_identity,removed,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET file_identity=excluded.file_identity,removed=excluded.removed,payload=excluded.payload", params![v.id,v.file_identity,v.removed,encode(v)?]).map(drop).map_err(failed)
    }
    fn save_registration_if_revision(
        &self,
        value: &Registration,
        revision: u64,
    ) -> Result<bool, StorageError> {
        let value = &registration_paths(value.clone());
        let changed = crate::db::lock(&self.conn).execute("UPDATE manual_registrations SET payload=?2 WHERE id=?1 AND removed=0 AND json_extract(payload,'$.revision')=?3", params![value.id, encode(value)?, i64::try_from(revision).map_err(failed)?]).map_err(failed)?;
        Ok(changed == 1)
    }
    fn watches(&self) -> Result<Vec<WatchRoot>, StorageError> {
        self.list("SELECT payload FROM discovery_watches ORDER BY id")
            .map(|rows| rows.into_iter().map(watch_paths).collect())
    }
    fn save_watch(&self, v: &WatchRoot) -> Result<(), StorageError> {
        let v = &watch_paths(v.clone());
        crate::db::lock(&self.conn).execute("INSERT INTO discovery_watches(id,payload) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![v.id,encode(v)?]).map(drop).map_err(failed)
    }
    fn candidates(&self) -> Result<Vec<Candidate>, StorageError> {
        self.list("SELECT payload FROM discovery_candidates ORDER BY fingerprint")
            .map(|rows| rows.into_iter().map(candidate_paths).collect())
    }
    fn save_candidate(&self, v: &Candidate) -> Result<(), StorageError> {
        let v = &candidate_paths(v.clone());
        crate::db::lock(&self.conn).execute("INSERT INTO discovery_candidates(fingerprint,payload) VALUES(?1,?2) ON CONFLICT(fingerprint) DO UPDATE SET payload=excluded.payload", params![v.fingerprint,encode(v)?]).map(drop).map_err(failed)
    }
    fn inferences(&self, account: &str) -> Result<Vec<Inference>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let mut statement = conn
            .prepare(
                "SELECT payload FROM local_inferences WHERE account_id=?1 ORDER BY installation_id",
            )
            .map_err(failed)?;
        let rows = statement
            .query_map([account], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        rows.map(|row| serde_json::from_str(&row.map_err(failed)?).map_err(failed))
            .collect()
    }
    fn save_inference(&self, v: &Inference) -> Result<(), StorageError> {
        crate::db::lock(&self.conn).execute("INSERT INTO local_inferences(account_id,installation_id,payload) SELECT ?1,?2,?3 WHERE EXISTS (SELECT 1 FROM manual_registrations WHERE id=?2 AND removed=0 AND json_extract(payload,'$.revision')=json_extract(?3,'$.revision')) ON CONFLICT(account_id,installation_id) DO UPDATE SET payload=excluded.payload", params![v.account,v.installation,encode(v)?]).map(drop).map_err(failed)
    }
    fn profiles(&self) -> Result<Vec<SaveProfile>, StorageError> {
        self.list("SELECT payload FROM save_profiles ORDER BY rowid")
    }
    fn save_profile(&self, v: &SaveProfile) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        // Cancel queued work with frozen, now obsolete rules. Preserve history and running jobs.
        let mut jobs = tx
            .prepare("SELECT payload FROM backup_jobs WHERE installation_id=?1 AND state='Queued'")
            .map_err(failed)?;
        let rows = jobs
            .query_map([&v.installation], |row| row.get::<_, String>(0))
            .map_err(failed)?;
        let mut stale = Vec::new();
        for row in rows {
            let mut job: BackupJob = serde_json::from_str(&row.map_err(failed)?).map_err(failed)?;
            if job.profile != v.id || job.revision != v.revision {
                job.state = JobState::Cancelled;
                stale.push(job);
            }
        }
        drop(jobs);
        for job in stale {
            tx.execute(
                "UPDATE backup_jobs SET state='Cancelled',payload=?2 WHERE id=?1",
                params![job.id, encode(&job)?],
            )
            .map_err(failed)?;
        }
        tx.execute("INSERT INTO save_profiles(id,installation_id,revision,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,payload=excluded.payload",params![v.id,v.installation,i64::try_from(v.revision).map_err(failed)?,encode(v)?]).map_err(failed)?;
        tx.commit().map_err(failed)
    }
    fn jobs(&self) -> Result<Vec<BackupJob>, StorageError> {
        self.list("SELECT payload FROM backup_jobs ORDER BY id")
    }
    fn save_job(&self, v: &BackupJob) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "INSERT OR IGNORE INTO backup_destinations(path) VALUES(?1)",
            [&v.destination],
        )
        .map_err(failed)?;
        tx.execute("INSERT INTO backup_jobs(id,installation_id,profile_id,revision,play_period,destination,state,payload) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET state=excluded.state,payload=excluded.payload",params![v.id,v.installation,v.profile,i64::try_from(v.revision).map_err(failed)?,v.play_period,v.destination,format!("{:?}",v.state),encode(v)?]).map_err(failed)?;
        tx.commit().map_err(failed)
    }
    fn backup_settings(&self) -> Result<BackupSettings, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let raw: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key='backup_settings'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(failed)?;
        raw.map(|s| serde_json::from_str(&s).map_err(failed))
            .unwrap_or_else(|| Ok(BackupSettings::default()))
    }
    fn save_backup_settings(&self, v: &BackupSettings) -> Result<(), StorageError> {
        let mut conn = crate::db::lock(&self.conn);
        let tx = conn.transaction().map_err(failed)?;
        tx.execute("INSERT INTO settings(key,value) VALUES('backup_settings',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",[encode(v)?]).map_err(failed)?;
        if !v.destination.is_empty() {
            tx.execute(
                "INSERT OR IGNORE INTO backup_destinations(path) VALUES(?1)",
                [&v.destination],
            )
            .map_err(failed)?;
        }
        tx.commit().map_err(failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registration() -> Registration {
        Registration {
            id: "stable".into(),
            title: "Game".into(),
            executable: "C:/Game/game.exe".into(),
            file_identity: "volume:file".into(),
            root: "C:/Game".into(),
            working_directory: "C:/Game".into(),
            arguments: vec![],
            gameplay: vec![],
            launchers: vec![],
            revision: 1,
            removed: false,
            rediscovery_allowed: false,
            transferred_from: None,
            available: true,
        }
    }
    #[test]
    fn legacy_verbatim_paths_are_cleaned_on_read_and_write() {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        let mut game = registration();
        game.root = r"\\?\F:\Games\Game".into();
        game.executable = format!(r"{}\game.exe", game.root);
        game.working_directory = game.root.clone();
        game.gameplay = vec![game.executable.clone()];
        game.launchers = vec![format!(r"{}\launcher.exe", game.root)];
        let watch = WatchRoot {
            id: "watch".into(),
            path: r"\\?\UNC\nas\games".into(),
            enabled: true,
        };
        let candidate = Candidate {
            fingerprint: "stable-fingerprint".into(),
            watch: watch.id.clone(),
            root: game.root.clone(),
            title: game.title.clone(),
            targets: game.gameplay.clone(),
            selected_target: Some(game.executable.clone()),
            evidence: vec![],
            ignored: true,
            incomplete: false,
        };
        // Insert the old payloads directly, bypassing the corrected write path.
        conn.execute(
            "INSERT INTO manual_registrations VALUES(?1,?2,?3,?4)",
            params![
                game.id,
                game.file_identity,
                game.removed,
                encode(&game).unwrap()
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO discovery_watches VALUES(?1,?2)",
            params![watch.id, encode(&watch).unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO discovery_candidates VALUES(?1,?2)",
            params![candidate.fingerprint, encode(&candidate).unwrap()],
        )
        .unwrap();
        let store = SqliteLocalGameStore::new(Arc::new(Mutex::new(conn)));
        let clean = store.registrations().unwrap().remove(0);
        assert_eq!(clean.root, r"F:\Games\Game");
        assert_eq!(clean.executable, r"F:\Games\Game\game.exe");
        assert_eq!(clean.working_directory, clean.root);
        assert_eq!(clean.gameplay, vec![clean.executable.clone()]);
        assert_eq!(clean.launchers, vec![r"F:\Games\Game\launcher.exe"]);
        assert_eq!((clean.id, clean.revision), (game.id.clone(), game.revision));
        assert_eq!(store.watches().unwrap()[0].path, r"\\nas\games");
        let clean_candidate = store.candidates().unwrap().remove(0);
        assert_eq!(clean_candidate.root, r"F:\Games\Game");
        assert_eq!(clean_candidate.targets, vec![r"F:\Games\Game\game.exe"]);
        assert_eq!(
            clean_candidate.selected_target.as_deref(),
            Some(r"F:\Games\Game\game.exe")
        );
        assert!(clean_candidate.ignored);
        store.save_registration(&game).unwrap();
        assert!(
            store
                .save_registration_if_revision(&game, game.revision)
                .unwrap()
        );
        store.save_watch(&watch).unwrap();
        store.save_candidate(&candidate).unwrap();
        let conn = crate::db::lock(&store.conn);
        for table in [
            "manual_registrations",
            "discovery_watches",
            "discovery_candidates",
        ] {
            let payload: String = conn
                .query_row(&format!("SELECT payload FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert!(!payload.contains('?'), "{payload}");
        }
    }

    #[test]
    fn upgrade_duplicate_repair_and_tombstone() {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrate(&mut conn, &crate::db::MIGRATIONS[..3]).unwrap();
        conn.execute(
            "UPDATE settings SET value='software' WHERE key='renderer'",
            [],
        )
        .unwrap();
        crate::db::migrate(&mut conn, crate::db::MIGRATIONS).unwrap();
        assert_eq!(
            conn.query_row("SELECT value FROM settings WHERE key='renderer'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "software"
        );
        let store = SqliteLocalGameStore::new(Arc::new(Mutex::new(conn)));
        let mut a = registration();
        store.save_registration(&a).unwrap();
        let mut duplicate = a.clone();
        duplicate.id = "other".into();
        assert!(store.save_registration(&duplicate).is_err());
        a.repair("C:/Moved/game.exe".into(), "volume:new".into());
        store.save_registration(&a).unwrap();
        assert_eq!(store.registrations().unwrap(), vec![a.clone()]);
        a.removed = true;
        store.save_registration(&a).unwrap();
        assert!(store.registrations().unwrap()[0].removed);
    }
    #[test]
    fn jobs_survive_reopen_and_profile_changes_cancel_only_queued_work() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.db");
        let settings = crate::db::SqliteSettingsStore::open(&path).unwrap();
        let store = settings.local_game_store();
        let profile = SaveProfile {
            id: "profile".into(),
            installation: "stable".into(),
            revision: 1,
            title: "Game".into(),
            manifest_hash: "hash".into(),
            definition: "{}".into(),
            files: vec![],
            registry: vec![],
            support: SaveSupport::Supported,
            automatic: None,
        };
        store.save_profile(&profile).unwrap();
        let job = BackupJob {
            id: "job".into(),
            installation: "stable".into(),
            profile: "profile".into(),
            revision: 1,
            play_period: Some("period".into()),
            destination: "D:/archives".into(),
            state: JobState::Queued,
            attempts: 0,
            ready_at: 10,
            backup_id: None,
            error: None,
        };
        store.save_job(&job).unwrap();
        drop(store);
        drop(settings);
        let settings = crate::db::SqliteSettingsStore::open(&path).unwrap();
        let store = settings.local_game_store();
        assert_eq!(store.jobs().unwrap(), vec![job]);
        let mut updated = profile;
        updated.revision = 2;
        store.save_profile(&updated).unwrap();
        assert_eq!(store.jobs().unwrap()[0].state, JobState::Cancelled);
        let inferred = Inference {
            installation: "stable".into(),
            revision: 1,
            title: "Game".into(),
            evidence: vec![],
            game_id: Some(5),
            account: "a".into(),
        };
        store.save_inference(&inferred).unwrap();
        assert!(store.inferences("b").unwrap().is_empty());
        assert_eq!(store.profiles().unwrap(), vec![updated]);
        assert!(store.backup_settings().unwrap().automatic);
    }
}
