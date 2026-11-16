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
pub(crate) fn registration_paths(mut value: Registration) -> Registration {
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
    fn save_candidate_suggestion(
        &self,
        fingerprint: &str,
        suggestion: Option<&gamenode_core::library::SuggestionView>,
    ) -> Result<(), StorageError> {
        // One JSON update keeps a suggestion write from overwriting a newer selected target or
        // any other field another worker changed while the search was in flight.
        let suggestion = serde_json::to_string(&suggestion).map_err(failed)?;
        crate::db::lock(&self.conn)
            .execute(
                "UPDATE discovery_candidates SET payload=json_set(payload,'$.suggestion',json(?2)) WHERE fingerprint=?1",
                params![fingerprint, suggestion],
            )
            .map(drop)
            .map_err(failed)
    }
    fn delete_candidate(&self, fingerprint: &str) -> Result<(), StorageError> {
        crate::db::lock(&self.conn)
            .execute(
                "DELETE FROM discovery_candidates WHERE fingerprint=?1",
                params![fingerprint],
            )
            .map(drop)
            .map_err(failed)
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
    fn backup_version_meta(
        &self,
        installation: &str,
    ) -> Result<Vec<BackupVersionMeta>, StorageError> {
        let conn = crate::db::lock(&self.conn);
        let mut statement = conn
            .prepare(
                "SELECT installation, profile, backup_id, reason, play_seconds, created_at
                 FROM backup_version_meta WHERE installation=?1 ORDER BY created_at, backup_id",
            )
            .map_err(failed)?;
        let rows = statement
            .query_map([installation], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(failed)?;
        let mut meta = Vec::new();
        for row in rows {
            let (installation, profile, backup_id, reason, play_seconds, created_at) =
                row.map_err(failed)?;
            let Some(reason) = BackupReason::parse(&reason) else {
                continue;
            };
            meta.push(BackupVersionMeta {
                installation,
                profile,
                backup_id,
                reason,
                play_seconds: play_seconds.and_then(|value| value.try_into().ok()),
                created_at,
            });
        }
        Ok(meta)
    }
    fn save_backup_version_meta(&self, v: &BackupVersionMeta) -> Result<(), StorageError> {
        crate::db::lock(&self.conn)
            .execute(
                "INSERT INTO backup_version_meta
                    (installation, profile, backup_id, reason, play_seconds, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(installation, profile, backup_id) DO UPDATE SET
                    reason=excluded.reason,
                    play_seconds=excluded.play_seconds,
                    created_at=excluded.created_at",
                params![
                    v.installation,
                    v.profile,
                    v.backup_id,
                    v.reason.as_str(),
                    v.play_seconds.map(i64::from),
                    v.created_at
                ],
            )
            .map(drop)
            .map_err(failed)
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
            custom_cover: None,
            available: true,
        }
    }
    #[test]
    fn a_custom_cover_round_trips_with_the_registration() {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        let store = SqliteLocalGameStore::new(Arc::new(Mutex::new(conn)));
        let mut game = registration();
        game.custom_cover = Some("local-stable-0123456789abcdef.webp".into());
        store.save_registration(&game).unwrap();
        assert_eq!(
            store.registrations().unwrap()[0].custom_cover.as_deref(),
            Some("local-stable-0123456789abcdef.webp")
        );
        let mut cleared = game.clone();
        cleared.custom_cover = None;
        cleared.revision = 2;
        store.save_registration(&cleared).unwrap();
        assert_eq!(store.registrations().unwrap()[0].custom_cover, None);
    }
    #[test]
    fn a_candidate_round_trips_review_fields_and_can_be_deleted() {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        let store = SqliteLocalGameStore::new(Arc::new(Mutex::new(conn)));
        let candidate = Candidate {
            fingerprint: "fingerprint".into(),
            watch: "watch".into(),
            root: "C:/Games/Instaladores".into(),
            title: "setup_redist".into(),
            targets: vec!["C:/Games/Instaladores/vc_redist.x64.exe".into()],
            selected_target: Some("C:/Games/Instaladores/vc_redist.x64.exe".into()),
            evidence: vec![],
            ignored: false,
            incomplete: false,
            suggestion: Some(gamenode_core::library::SuggestionView {
                game_id: 42,
                title: "Moss & Machine".into(),
                year: Some(2023),
                cover_url: None,
                confidence: 96,
                tone: gamenode_core::library::SuggestionTone::Success,
            }),
            likely_non_game: true,
        };
        store.save_candidate(&candidate).unwrap();
        let saved = store.candidates().unwrap().remove(0);
        assert_eq!(saved.suggestion, candidate.suggestion);
        assert!(saved.likely_non_game);
        // A suggestion update is targeted and keeps the rest of the record.
        store
            .save_candidate_suggestion("fingerprint", None)
            .unwrap();
        let saved = store.candidates().unwrap().remove(0);
        assert_eq!(saved.suggestion, None);
        assert_eq!(saved.selected_target, candidate.selected_target);
        store
            .save_candidate_suggestion("fingerprint", candidate.suggestion.as_ref())
            .unwrap();
        assert_eq!(
            store.candidates().unwrap().remove(0).suggestion,
            candidate.suggestion
        );
        store.delete_candidate("fingerprint").unwrap();
        assert!(store.candidates().unwrap().is_empty());
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
            suggestion: None,
            likely_non_game: false,
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

    /// Payloads written before the camelCase serde change use snake_case keys and PascalCase enum
    /// variants. Every persisted record must still load: one unreadable row fails the whole
    /// local-games view, which then shows no watched folders and no DRM-free games.
    #[test]
    fn pre_migration_snake_case_payloads_still_load() {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        conn.execute(
            "INSERT INTO manual_registrations(id,file_identity,removed,payload) VALUES('legacy-game','vol:1',0,?1)",
            [r#"{"id":"legacy-game","title":"Elden Ring","executable":"F:\\Games\\ER\\eldenring.exe","file_identity":"vol:1","root":"F:\\Games\\ER","working_directory":"F:\\Games\\ER","arguments":[],"gameplay":["F:\\Games\\ER\\eldenring.exe"],"launchers":[],"revision":3,"removed":false,"rediscovery_allowed":true,"transferred_from":"old-id","available":true}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO discovery_watches(id,payload) VALUES('legacy-watch',?1)",
            [r#"{"id":"legacy-watch","path":"F:\\Games","enabled":true}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO discovery_candidates(fingerprint,payload) VALUES('legacy-fp',?1)",
            [r#"{"fingerprint":"legacy-fp","watch":"legacy-watch","root":"F:\\Games\\Hades","title":"Hades","targets":["F:\\Games\\Hades\\Hades.exe"],"selected_target":"F:\\Games\\Hades\\Hades.exe","evidence":[{"kind":"Product","value":"Hades","strong":true},{"kind":"ExternalId","value":"1145360","strong":false}],"ignored":false,"incomplete":false}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO local_inferences(account_id,installation_id,payload) VALUES('acct-1','legacy-game',?1)",
            [r#"{"installation":"legacy-game","revision":3,"title":"Elden Ring","evidence":[{"kind":"Executable","value":"eldenring.exe","strong":true}],"game_id":42,"account":"acct-1"}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO save_profiles(id,installation_id,revision,payload) VALUES('p1','legacy-game',3,?1)",
            [r#"{"id":"p1","installation":"legacy-game","revision":3,"title":"Elden Ring saves","manifest_hash":"h1","definition":"d","files":["a"],"registry":[],"support":"NeedsSelection","automatic":true}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO backup_jobs(id,installation_id,profile_id,revision,play_period,destination,state,payload) VALUES('j1','legacy-game','p1',3,'2026-01','D:\\Backups','Queued',?1)",
            [r#"{"id":"j1","installation":"legacy-game","profile":"p1","revision":3,"play_period":"2026-01","destination":"D:\\Backups","state":"Queued","attempts":0,"ready_at":0,"backup_id":null,"error":null}"#],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO settings(key,value) VALUES('backup_settings',?1)",
            [r#"{"automatic":true,"destination":"D:\\Backups"}"#],
        )
        .unwrap();

        let store = SqliteLocalGameStore::new(Arc::new(Mutex::new(conn)));
        let registrations = store.registrations().expect("legacy registrations load");
        assert_eq!(registrations.len(), 1);
        let game = &registrations[0];
        assert_eq!(game.file_identity, "vol:1");
        assert_eq!(game.working_directory, r"F:\Games\ER");
        assert!(game.rediscovery_allowed);
        assert_eq!(game.transferred_from.as_deref(), Some("old-id"));
        assert_eq!(game.custom_cover, None);

        assert_eq!(
            store.watches().expect("legacy watches load")[0].id,
            "legacy-watch"
        );

        let candidate = store
            .candidates()
            .expect("legacy candidates load")
            .remove(0);
        assert_eq!(
            candidate.selected_target.as_deref(),
            Some(r"F:\Games\Hades\Hades.exe")
        );
        let kinds: Vec<_> = candidate.evidence.iter().map(|e| e.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                gamenode_core::drm_free::EvidenceKind::Product,
                gamenode_core::drm_free::EvidenceKind::ExternalId,
            ]
        );

        let inference = store
            .inferences("acct-1")
            .expect("legacy inferences load")
            .remove(0);
        assert_eq!(inference.game_id, Some(42));
        assert_eq!(
            inference.evidence[0].kind,
            gamenode_core::drm_free::EvidenceKind::Executable
        );

        let profile = store.profiles().expect("legacy profiles load").remove(0);
        assert_eq!(
            profile.support,
            gamenode_core::drm_free::SaveSupport::NeedsSelection
        );
        assert_eq!(profile.manifest_hash, "h1");

        let job = store.jobs().expect("legacy jobs load").remove(0);
        assert_eq!(job.state, gamenode_core::drm_free::JobState::Queued);
        assert_eq!(job.play_period.as_deref(), Some("2026-01"));

        let settings = store.backup_settings().expect("legacy settings load");
        assert_eq!(settings.destination, r"D:\Backups");
        assert_eq!(settings.retention, 10);
    }

    #[test]
    fn backup_version_meta_round_trips_and_replaces_a_row() {
        let conn = crate::db::open(std::path::Path::new(":memory:")).unwrap();
        let store = SqliteLocalGameStore::new(Arc::new(Mutex::new(conn)));
        let first = BackupVersionMeta {
            installation: "game".into(),
            profile: "profile".into(),
            backup_id: "backup-1".into(),
            reason: BackupReason::AfterSession,
            play_seconds: Some(42 * 60),
            created_at: 100,
        };
        let recovery = BackupVersionMeta {
            installation: "game".into(),
            profile: "profile_recovery_1".into(),
            backup_id: "backup-1".into(),
            reason: BackupReason::Recovery,
            play_seconds: None,
            created_at: 200,
        };
        let other = BackupVersionMeta {
            installation: "other".into(),
            ..first.clone()
        };
        store.save_backup_version_meta(&first).unwrap();
        store.save_backup_version_meta(&recovery).unwrap();
        store.save_backup_version_meta(&other).unwrap();
        let meta = store.backup_version_meta("game").unwrap();
        assert_eq!(meta, vec![first.clone(), recovery.clone()]);
        assert_eq!(
            store.backup_version_meta("other").unwrap(),
            vec![other.clone()]
        );

        let updated = BackupVersionMeta {
            reason: BackupReason::Manual,
            play_seconds: None,
            ..first
        };
        store.save_backup_version_meta(&updated).unwrap();
        assert_eq!(store.backup_version_meta("game").unwrap()[0], updated);
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
            retention: None,
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
            play_seconds: None,
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
