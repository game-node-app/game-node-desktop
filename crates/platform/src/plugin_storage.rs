//! Durable private key/value storage of third-party plugins.
//!
//! Every plugin has one store per GameNode account profile, in its own SQLite file below
//! `plugin-data/<publisher fingerprint>/<plugin id>/<profile id>/`. The host decides the
//! directory from the authenticated identity ([`namespace_dir`]); a plugin never names a path or
//! another namespace, and nothing here returns one. A store is opened on a blocking worker, and
//! each [`DurableStore::put`] is one transaction, so a write that would pass the quota changes
//! nothing and a crash leaves either the old value or the new one.
//!
//! The quota counts the logical bytes (key, value and a fixed per-entry overhead), not the
//! file size. Revisions are handed in by the caller, who keeps them growing across deletions and
//! restarts through [`DurableStore::high_water`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

/// The longest key, in bytes.
pub const MAX_KEY_BYTES: usize = 256;
/// The largest value, in bytes.
pub const MAX_VALUE_BYTES: usize = 1024 * 1024;
/// The logical bytes a plugin may keep durably per profile.
pub const QUOTA_BYTES: u64 = 16 * 1024 * 1024;
/// Charged per entry on top of its key and value, so many tiny entries cannot dodge the quota.
pub const ENTRY_OVERHEAD_BYTES: u64 = 64;
/// The longest an entry may live before it expires (30 days).
pub const MAX_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

const SCHEMA_VERSION: i64 = 16;
const STORE_FILE: &str = "storage.sqlite3";
/// Longest directory name derived from an identity before it is replaced by a digest.
const MAX_COMPONENT_BYTES: usize = 120;

/// Why the store could not be used. Messages never contain a path.
#[derive(Debug, thiserror::Error)]
pub enum PluginStorageError {
    #[error("the plugin storage could not be opened")]
    Open(#[source] io::Error),
    #[error("the plugin storage failed")]
    Database(#[source] rusqlite::Error),
    #[error("the plugin storage was written by a newer version of GameNode")]
    NewerVersion,
    #[error("the entry is protected by the user")]
    Protected,
    #[error("the entry is archived and needs a reviewed reuse")]
    Archived,
    #[error("a publisher migration needs two different identities")]
    InvalidMigration,
}

impl From<rusqlite::Error> for PluginStorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

/// What the quota charges for one entry.
pub fn charge(key: &str, value_len: usize) -> u64 {
    key.len() as u64 + value_len as u64 + ENTRY_OVERHEAD_BYTES
}

/// A directory name derived from one identity part: readable characters stay, anything else
/// becomes `%XX`, and a name that would be too long or only dots is replaced by a digest.
fn component(part: &str) -> String {
    let mut name = String::with_capacity(part.len());
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_')
            || (byte == b'.' && !name.is_empty())
        {
            name.push(byte as char);
        } else {
            name.push_str(&format!("%{byte:02X}"));
        }
    }
    if name.is_empty() || name.len() > MAX_COMPONENT_BYTES || name.ends_with('.') {
        let digest = Sha256::digest(part.as_bytes());
        let hex: String = digest[..16].iter().map(|b| format!("{b:02x}")).collect();
        return format!("h-{hex}");
    }
    name
}

/// The directory of one plugin's store in one profile below `root` (`plugin-data`). Pure: it
/// creates nothing.
pub fn namespace_dir(root: &Path, publisher: &str, plugin: &str, profile: &str) -> PathBuf {
    root.join(component(publisher))
        .join(component(plugin))
        .join(component(profile))
}

/// The SQLite file of one plugin's store in one profile below `root` (`plugin-data`). Pure: it
/// creates nothing. Host-only administration uses this to measure the file; no plugin reply ever
/// carries it.
pub fn namespace_store_file(root: &Path, publisher: &str, plugin: &str, profile: &str) -> PathBuf {
    namespace_dir(root, publisher, plugin, profile).join(STORE_FILE)
}

/// Archives one namespace's live private entries without creating a store. `None` means the
/// namespace never wrote anything durably. Used by bundle uninstall, which has no live
/// [`PluginStorage`] worker at that point.
pub fn archive_namespace(
    root: &Path,
    publisher: &str,
    plugin: &str,
    profile: &str,
) -> Result<Option<PrivateArchiveOutcome>, PluginStorageError> {
    let dir = namespace_dir(root, publisher, plugin, profile);
    let Some(mut store) = DurableStore::open_existing(&dir)? else {
        return Ok(None);
    };
    let revision = store.high_water()?.saturating_add(1);
    store.archive_entries(revision).map(Some)
}

/// Reads one namespace's logical usage without creating a store. `None` means the namespace never
/// wrote anything durably. Values and the namespace path are never returned.
pub fn namespace_usage(
    root: &Path,
    publisher: &str,
    plugin: &str,
    profile: &str,
    now: i64,
) -> Result<Option<StoreUsage>, PluginStorageError> {
    let dir = namespace_dir(root, publisher, plugin, profile);
    match DurableStore::open_existing(&dir)? {
        Some(store) => store.store_usage(now).map(Some),
        None => Ok(None),
    }
}

/// Reads the archived entries of one namespace without creating a store.
pub fn namespace_archived_usage(
    root: &Path,
    publisher: &str,
    plugin: &str,
    profile: &str,
) -> Result<Option<PrivateArchiveOutcome>, PluginStorageError> {
    let dir = namespace_dir(root, publisher, plugin, profile);
    match DurableStore::open_existing(&dir)? {
        Some(store) => store.archived_usage().map(Some),
        None => Ok(None),
    }
}

/// One source entry as read for a migration copy. Values never leave this module.
struct ExportEntry {
    key: String,
    value: Vec<u8>,
    expires_at: Option<i64>,
    last_used_at: i64,
    user_protected: bool,
}

fn export_entries(conn: &Connection) -> Result<Vec<ExportEntry>, PluginStorageError> {
    let mut statement = conn.prepare(
        "SELECT key, value, expires_at, last_used_at, user_protected
         FROM entries WHERE archived = 0",
    )?;
    let rows = statement.query_map([], |row| {
        Ok(ExportEntry {
            key: row.get(0)?,
            value: row.get(1)?,
            expires_at: row.get(2)?,
            last_used_at: row.get(3)?,
            user_protected: row.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn stored_keys(conn: &Connection) -> Result<std::collections::HashSet<String>, PluginStorageError> {
    let mut statement = conn.prepare("SELECT key FROM entries")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.collect::<Result<std::collections::HashSet<_>, _>>()
        .map_err(Into::into)
}

/// Reviews the private data a re-trusted publisher identity could receive from its predecessor.
///
/// Only the old identity's live, unexpired entries are candidates. Expired rows and rows retained
/// by an uninstall are reported, never copied: the latter need the separate reviewed reuse. A key
/// the new identity already holds is a conflict. Values and paths are never returned and nothing is
/// created or changed.
pub fn migration_preview(
    root: &Path,
    source_publisher: &str,
    source_plugin: &str,
    source_profile: &str,
    target_publisher: &str,
    target_plugin: &str,
    target_profile: &str,
    now: i64,
) -> Result<PrivateMigrationPreview, PluginStorageError> {
    let source_dir = namespace_dir(root, source_publisher, source_plugin, source_profile);
    let target_dir = namespace_dir(root, target_publisher, target_plugin, target_profile);
    if source_dir == target_dir {
        return Err(PluginStorageError::InvalidMigration);
    }
    let mut preview = PrivateMigrationPreview::default();
    let target_keys = match DurableStore::open_existing(&target_dir)? {
        Some(store) => {
            let usage = store.store_usage(now)?;
            preview.target_records = usage
                .categories
                .iter()
                .map(|category| category.records)
                .sum::<u64>()
                .saturating_add(usage.archived_records);
            preview.target_logical_bytes = usage.logical_bytes;
            stored_keys(&store.conn)?
        }
        None => Default::default(),
    };
    let Some(source_store) = DurableStore::open_existing(&source_dir)? else {
        return Ok(preview);
    };
    preview.source_present = true;
    preview.archived_records = source_store.archived_usage()?.records;
    for entry in export_entries(&source_store.conn)? {
        if entry.expires_at.is_some_and(|at| at <= now) {
            preview.expired_records += 1;
            continue;
        }
        let size = charge(&entry.key, entry.value.len());
        preview.records += 1;
        preview.logical_bytes = preview.logical_bytes.saturating_add(size);
        if entry.user_protected {
            preview.protected_records += 1;
        }
        if target_keys.contains(&entry.key) {
            preview.conflict_records += 1;
        } else {
            preview.eligible_records += 1;
            preview.eligible_bytes = preview.eligible_bytes.saturating_add(size);
        }
    }
    Ok(preview)
}

/// Copies one private namespace's live entries to a re-trusted publisher identity in one
/// transaction.
///
/// Keys the new identity already holds are skipped as conflicts and expired rows are not copied;
/// archived rows stay behind for the separate reuse review. The source keeps every row, and each
/// copy records the old publisher fingerprint so a reviewed rollback removes exactly these copies.
/// Blocking: call this on the storage worker.
pub fn migrate_namespace(
    root: &Path,
    source_publisher: &str,
    source_plugin: &str,
    source_profile: &str,
    target_publisher: &str,
    target_plugin: &str,
    target_profile: &str,
    now: i64,
) -> Result<PrivateMigrationOutcome, PluginStorageError> {
    let source_dir = namespace_dir(root, source_publisher, source_plugin, source_profile);
    let target_dir = namespace_dir(root, target_publisher, target_plugin, target_profile);
    if source_dir == target_dir {
        return Err(PluginStorageError::InvalidMigration);
    }
    let Some(source_store) = DurableStore::open_existing(&source_dir)? else {
        return Ok(PrivateMigrationOutcome::default());
    };
    let entries = export_entries(&source_store.conn)?;
    let archived_records = source_store.archived_usage()?.records;
    let mut target = DurableStore::open(&target_dir)?;
    let mut revision = target.high_water()?;
    let mut outcome = PrivateMigrationOutcome {
        source_present: true,
        archived_records,
        ..PrivateMigrationOutcome::default()
    };
    let tx = target
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut keys = stored_keys(&tx)?;
    for entry in entries {
        if entry.expires_at.is_some_and(|at| at <= now) {
            outcome.expired_records += 1;
            continue;
        }
        if keys.contains(&entry.key) {
            outcome.conflict_records += 1;
            continue;
        }
        revision = revision.saturating_add(1);
        let size = charge(&entry.key, entry.value.len());
        tx.execute(
            "INSERT INTO entries
                 (key, value, revision, expires_at, last_used_at, user_protected, migrated_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.key.as_str(),
                entry.value.as_slice(),
                revision as i64,
                entry.expires_at,
                entry.last_used_at,
                i64::from(entry.user_protected),
                source_publisher,
            ],
        )?;
        keys.insert(entry.key);
        outcome.copied_records += 1;
        outcome.copied_bytes = outcome.copied_bytes.saturating_add(size);
    }
    if outcome.copied_records > 0 {
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
    }
    tx.commit()?;
    Ok(outcome)
}

/// Removes one migration's copies from the new publisher identity after a reviewed rollback.
///
/// Only entries tagged with the old fingerprint are removed, and a copy the user protected since
/// the migration stays. The old identity's original namespace is never touched. `None` means the
/// new identity never wrote a durable store. Blocking: call this on the storage worker.
pub fn revert_migrated_namespace(
    root: &Path,
    target_publisher: &str,
    target_plugin: &str,
    target_profile: &str,
    source_publisher: &str,
) -> Result<Option<PrivateMigrationRevert>, PluginStorageError> {
    let dir = namespace_dir(root, target_publisher, target_plugin, target_profile);
    let Some(mut store) = DurableStore::open_existing(&dir)? else {
        return Ok(None);
    };
    let revision = store.high_water()?.saturating_add(1);
    let tx = store
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (records, bytes): (i64, i64) = tx.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?2), 0)
         FROM entries WHERE migrated_from = ?1 AND user_protected = 0",
        params![source_publisher, ENTRY_OVERHEAD_BYTES as i64],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let removed = tx.execute(
        "DELETE FROM entries WHERE migrated_from = ?1 AND user_protected = 0",
        [source_publisher],
    )? as u64;
    let kept: i64 = tx.query_row(
        "SELECT COUNT(*) FROM entries WHERE migrated_from = ?1 AND user_protected = 1",
        [source_publisher],
        |row| row.get(0),
    )?;
    if removed > 0 {
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
    }
    tx.commit()?;
    Ok(Some(PrivateMigrationRevert {
        removed_records: records.max(0) as u64,
        reclaimed_bytes: bytes.max(0) as u64,
        kept_protected_records: kept.max(0) as u64,
    }))
}

/// Removes one plugin/profile private namespace directory. Only that profile directory is
/// removed; the publisher/plugin directories and every other profile stay. A symlink or a
/// non-directory is refused instead of followed. Returns whether there was a directory.
pub fn remove_namespace(
    root: &Path,
    publisher: &str,
    plugin: &str,
    profile: &str,
) -> io::Result<bool> {
    let dir = namespace_dir(root, publisher, plugin, profile);
    if !dir.starts_with(root) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "private data path escapes the plugin-data root",
        ));
    }
    remove_namespace_at(&dir)
}

/// Removes one already-resolved private namespace directory with the same symlink refusal.
pub fn remove_namespace_at(dir: &Path) -> io::Result<bool> {
    let metadata = match fs::symlink_metadata(dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private data is not a regular directory",
        ));
    }
    fs::remove_dir_all(dir)?;
    Ok(true)
}

/// A stored value as read back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stored {
    pub value: Vec<u8>,
    pub revision: u64,
    /// Unix seconds, `None` when the entry does not expire.
    pub expires_at: Option<i64>,
}

/// Logical metadata for one durable private key; never includes value bytes or a filesystem path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistenceEntry {
    pub key: String,
    pub size_bytes: u64,
    pub last_used_at: i64,
    /// Set only by host administration. A plugin can neither set nor clear it.
    pub user_protected: bool,
}

/// Logical usage of one persistence category; never includes a value or a filesystem path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryUsage {
    pub category: String,
    pub records: u64,
    pub logical_bytes: u64,
    /// Records in this category the user protected from plugin and retention deletion.
    pub protected_records: u64,
}

/// Logical accounting of one plugin's durable private store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreUsage {
    /// The persistence categories the store currently exposes. Only the `private` category
    /// counts against the plugin quota; the list is shaped for future categories.
    pub categories: Vec<CategoryUsage>,
    /// The quota-charged logical bytes: the sum of `categories`.
    pub logical_bytes: u64,
    /// SQLite page allocation for the complete namespace database. SQLite metadata and free
    /// pages are shared across categories, so this is never a per-category physical figure.
    pub physical_bytes_estimate: u64,
    /// Entries archived by uninstall Keep. They are inert and excluded from `categories`.
    pub archived_records: u64,
    /// Logical bytes of the archived entries, not charged against the live quota.
    pub archived_logical_bytes: u64,
    /// The oldest last-use time among live entries (Unix seconds), if any.
    pub oldest_used_at: Option<i64>,
    /// The stored retention policy of the `private` category, if the user set one.
    pub retention: Option<RetentionPolicy>,
}

/// The result of archiving or restoring one owner's private entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateArchiveOutcome {
    pub records: u64,
    pub logical_bytes: u64,
}

/// The result of a host cleanup of expired and policy-exceeding records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanupOutcome {
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
}

/// User-approved retention limits for a logical storage category.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_age_seconds: u64,
    pub max_bytes: u64,
}

/// Logical metadata of one live private entry for a reviewed publisher migration. It never
/// carries a value or a filesystem path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrivateMigrationEntry {
    pub key: String,
    pub size_bytes: u64,
    pub expires_at: Option<i64>,
    pub last_used_at: i64,
    pub user_protected: bool,
}

/// What one private namespace of a re-trusted publisher holds for a migration preview. Values and
/// paths are never part of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateMigrationPreview {
    /// Whether the old identity ever wrote a durable private store.
    pub source_present: bool,
    /// Live, unexpired entries of the old identity.
    pub records: u64,
    pub logical_bytes: u64,
    /// Entries that can be copied: live, unexpired and absent from the new identity.
    pub eligible_records: u64,
    pub eligible_bytes: u64,
    /// Keys the new identity already holds (live or archived), so copying would overwrite them.
    pub conflict_records: u64,
    pub expired_records: u64,
    /// Rows retained by an uninstall; they need the separate reviewed reuse before migration.
    pub archived_records: u64,
    pub protected_records: u64,
    /// What the new identity already holds, for context. Never a physical size.
    pub target_records: u64,
    pub target_logical_bytes: u64,
}

/// The measured result of copying one private namespace to a re-trusted publisher identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PrivateMigrationOutcome {
    pub source_present: bool,
    pub copied_records: u64,
    pub copied_bytes: u64,
    pub conflict_records: u64,
    pub expired_records: u64,
    pub archived_records: u64,
}

/// The measured result of removing one migration's copies from the new identity. The old
/// identity's original entries are never touched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivateMigrationRevert {
    pub removed_records: u64,
    pub reclaimed_bytes: u64,
    /// Migrated entries the user protected since the migration; rollback keeps them.
    pub kept_protected_records: u64,
}

/// The result of a write that did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutResult {
    /// Stored. `usage` is the logical bytes the namespace now holds.
    Stored { usage: u64 },
    /// Nothing changed: the namespace would exceed the quota. `usage` is what it holds.
    OverQuota { usage: u64 },
}

/// One plugin's durable store in one profile.
pub struct DurableStore {
    conn: Connection,
}

impl DurableStore {
    /// Opens the store in `dir`, or `None` when this namespace never stored anything durably.
    /// Reading never creates files.
    pub fn open_existing(dir: &Path) -> Result<Option<Self>, PluginStorageError> {
        if !dir.join(STORE_FILE).is_file() {
            return Ok(None);
        }
        Self::open(dir).map(Some)
    }

    /// Opens the store in `dir`, creating the directories and the file.
    pub fn open(dir: &Path) -> Result<Self, PluginStorageError> {
        fs::create_dir_all(dir).map_err(PluginStorageError::Open)?;
        let conn = Connection::open(dir.join(STORE_FILE))?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(PluginStorageError::NewerVersion);
        }
        if version < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS meta (
                     id INTEGER PRIMARY KEY CHECK (id = 1),
                     revision INTEGER NOT NULL CHECK (revision >= 0)
                 ) STRICT;
                 INSERT OR IGNORE INTO meta (id, revision) VALUES (1, 0);
                 CREATE TABLE IF NOT EXISTS entries (
                     key TEXT PRIMARY KEY,
                     value BLOB NOT NULL,
                     revision INTEGER NOT NULL CHECK (revision >= 1),
                     expires_at INTEGER
                 ) STRICT;
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if version < 2 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS file_roots (
                     id TEXT NOT NULL UNIQUE,
                     installation_id TEXT NOT NULL,
                     game_id TEXT NOT NULL,
                     kind TEXT NOT NULL,
                     root TEXT NOT NULL,
                     root_volume INTEGER NOT NULL,
                     root_node INTEGER NOT NULL,
                     operations INTEGER NOT NULL CHECK (operations BETWEEN 1 AND 7),
                     affected TEXT NOT NULL,
                     approved_at INTEGER NOT NULL,
                     revoked_at INTEGER,
                     PRIMARY KEY (installation_id, kind)
                 ) STRICT;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if version < 3 {
            // How the approved folder was chosen: `user` for a folder the user picked or the
            // host suggested, `declared` for one the plugin suggested and the user took.
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE file_roots ADD COLUMN origin TEXT NOT NULL DEFAULT 'user';
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        if version < 4 {
            // Confirmations the user asked to remember (launch or install of one game by this
            // plugin). The row is the approval: revoking deletes it.
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS intent_approvals (
                     id TEXT NOT NULL UNIQUE,
                     game_id TEXT NOT NULL,
                     installation_id TEXT NOT NULL,
                     operation TEXT NOT NULL CHECK (operation IN ('launch', 'install')),
                     target_revision INTEGER NOT NULL CHECK (target_revision >= 0),
                     game_name TEXT NOT NULL,
                     approved_at INTEGER NOT NULL,
                     PRIMARY KEY (installation_id, operation)
                 ) STRICT;
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }
        if version < 5 {
            // Save profiles this plugin contributed: where a game's saves live for one runtime.
            // They reference folder approvals of the same namespace by id, never a path.
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE IF NOT EXISTS save_profiles (
                     id TEXT PRIMARY KEY,
                     installation_id TEXT NOT NULL,
                     game_id TEXT NOT NULL,
                     name TEXT NOT NULL,
                     name_key TEXT NOT NULL,
                     runtime_id TEXT NOT NULL,
                     format_id TEXT NOT NULL,
                     format_version INTEGER NOT NULL CHECK (format_version >= 0),
                     metadata TEXT NOT NULL,
                     directories TEXT NOT NULL,
                     revision INTEGER NOT NULL CHECK (revision >= 1),
                     created_at INTEGER NOT NULL,
                     UNIQUE (installation_id, name_key)
                 ) STRICT;
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }
        if version < 6 {
            conn.execute_batch("BEGIN IMMEDIATE; ALTER TABLE file_roots ADD COLUMN volume_token TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 6; COMMIT;")?;
        }
        if version < 7 {
            conn.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS plugin_libraries (id TEXT PRIMARY KEY, payload TEXT NOT NULL) STRICT; PRAGMA user_version = 7; COMMIT;")?;
        }
        if version < 8 {
            conn.execute_batch("BEGIN IMMEDIATE; CREATE TABLE IF NOT EXISTS backup_sources (profile_id TEXT NOT NULL, destination TEXT NOT NULL, PRIMARY KEY(profile_id,destination)) STRICT; PRAGMA user_version = 8; COMMIT;")?;
        }
        if version < 9 {
            // Custom launch consent is scoped to the exact persisted recipe revision. Existing
            // remembered launch/install consent has no recipe, so it migrates with revision 0.
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 ALTER TABLE intent_approvals RENAME TO intent_approvals_v8;
                 CREATE TABLE intent_approvals (
                     id TEXT NOT NULL UNIQUE,
                     game_id TEXT NOT NULL,
                     installation_id TEXT NOT NULL,
                     operation TEXT NOT NULL CHECK (operation IN ('launch', 'custom_launch', 'install')),
                     target_revision INTEGER NOT NULL CHECK (target_revision >= 0),
                     recipe_id TEXT NOT NULL,
                     recipe_revision INTEGER NOT NULL CHECK (recipe_revision >= 0),
                     game_name TEXT NOT NULL,
                     approved_at INTEGER NOT NULL,
                     PRIMARY KEY (installation_id, operation)
                 ) STRICT;
                 INSERT INTO intent_approvals
                     (id, game_id, installation_id, operation, target_revision,
                      recipe_id, recipe_revision, game_name, approved_at)
                 SELECT id, game_id, installation_id, operation, target_revision,
                        '', 0, game_name, approved_at
                 FROM intent_approvals_v8;
                 DROP TABLE intent_approvals_v8;
                 PRAGMA user_version = 9;
                 COMMIT;",
            )?;
        }
        if version < 10 {
            conn.execute_batch("BEGIN IMMEDIATE; ALTER TABLE intent_approvals ADD COLUMN plan_fingerprint TEXT NOT NULL DEFAULT ''; PRAGMA user_version = 10; COMMIT;")?;
        }
        if version < 11 {
            // A store whose version was set back for an upgrade test already has the column.
            let has_last_used_at = conn
                .prepare("PRAGMA table_info(entries)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "last_used_at");
            let add_column = if has_last_used_at {
                ""
            } else {
                "ALTER TABLE entries ADD COLUMN last_used_at INTEGER NOT NULL DEFAULT 0;"
            };
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;
                 {add_column}
                 CREATE TABLE IF NOT EXISTS retention_policies (
                     category TEXT PRIMARY KEY,
                     max_age_seconds INTEGER NOT NULL CHECK (max_age_seconds >= 0),
                     max_bytes INTEGER NOT NULL CHECK (max_bytes >= 0)
                 ) STRICT;
                 PRAGMA user_version = 11;
                 COMMIT;"
            ))?;
        }
        if version < 12 {
            // Shared emulator resources are declarations beside a profile's folders. Consent is
            // stored separately so a changed declaration or scope becomes stale without losing
            // the audit trail of what the user last reviewed.
            let has_shared_resources = conn
                .prepare("PRAGMA table_info(save_profiles)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "shared_resources");
            let add_column = if has_shared_resources {
                ""
            } else {
                "ALTER TABLE save_profiles ADD COLUMN shared_resources TEXT NOT NULL DEFAULT '[]';"
            };
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;
                 {add_column}
                 CREATE TABLE IF NOT EXISTS shared_resource_consents (
                     profile_id TEXT NOT NULL,
                     resource_id TEXT NOT NULL,
                     declaration TEXT NOT NULL,
                     scope TEXT NOT NULL,
                     profile_revision INTEGER NOT NULL CHECK (profile_revision >= 0),
                     approved_at INTEGER NOT NULL,
                     PRIMARY KEY (profile_id, resource_id)
                 ) STRICT;
                 PRAGMA user_version = 12;
                 COMMIT;"
            ))?;
        }
        if version < 13 {
            // Host-only user protection. Plugins can neither set nor clear it, and retention,
            // explicit deletion and cleanup must keep a protected record.
            let has_user_protected = conn
                .prepare("PRAGMA table_info(entries)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "user_protected");
            let add_column = if has_user_protected {
                ""
            } else {
                "ALTER TABLE entries ADD COLUMN user_protected INTEGER NOT NULL DEFAULT 0 CHECK (user_protected IN (0, 1));"
            };
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;
                 {add_column}
                 PRAGMA user_version = 13;
                 COMMIT;"
            ))?;
        }
        if version < 14 {
            // Host-only uninstall retention. An archived entry is inert: reads, lists, usage,
            // retention and cleanup skip it, and a plugin cannot silently replace it. Only the
            // reviewed host reuse clears the flag.
            let has_archived = conn
                .prepare("PRAGMA table_info(entries)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "archived");
            let add_column = if has_archived {
                ""
            } else {
                "ALTER TABLE entries ADD COLUMN archived INTEGER NOT NULL DEFAULT 0 CHECK (archived IN (0, 1));"
            };
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;
                 {add_column}
                 PRAGMA user_version = 14;
                 COMMIT;"
            ))?;
        }
        if version < 15 {
            // Host-only publisher migration. A copied entry names the old publisher fingerprint so
            // a reviewed rollback can remove exactly the copies without touching the original
            // namespace or a value the old identity never held. A plugin can neither set nor read
            // this marker.
            let has_migrated_from = conn
                .prepare("PRAGMA table_info(entries)")?
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()?
                .iter()
                .any(|name| name == "migrated_from");
            let add_column = if has_migrated_from {
                ""
            } else {
                "ALTER TABLE entries ADD COLUMN migrated_from TEXT;"
            };
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;
                 {add_column}
                 PRAGMA user_version = 15;
                 COMMIT;"
            ))?;
        }
        if version < 16 {
            // One exact executable the user approved for an owned runtime. The path and digest are
            // host-only; a plugin sees only the runtime record that references this row.
            conn.execute_batch(
                "BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS runtime_approvals (
                     id TEXT NOT NULL UNIQUE,
                     runtime_id TEXT PRIMARY KEY,
                     path TEXT NOT NULL,
                     volume INTEGER NOT NULL,
                     node INTEGER NOT NULL,
                     size INTEGER NOT NULL CHECK (size >= 0),
                     digest TEXT NOT NULL,
                     approved_at INTEGER NOT NULL,
                     revoked_at INTEGER
                 ) STRICT;
                 PRAGMA user_version = 16;
                 COMMIT;",
            )?;
        }
        Ok(Self { conn })
    }

    /// The highest revision this store ever recorded, deletions included.
    pub fn high_water(&self) -> Result<u64, PluginStorageError> {
        let revision: i64 =
            self.conn
                .query_row("SELECT revision FROM meta WHERE id = 1", [], |row| {
                    row.get(0)
                })?;
        Ok(revision as u64)
    }

    /// The entry under `key`, unless it expired at `now` (Unix seconds). An archived entry is
    /// invisible until a reviewed host reuse restores it.
    pub fn get(&self, key: &str, now: i64) -> Result<Option<Stored>, PluginStorageError> {
        let found = self
            .conn
            .query_row(
                "SELECT value, revision, expires_at FROM entries
                 WHERE key = ?1 AND archived = 0",
                [key],
                |row| {
                    Ok(Stored {
                        value: row.get(0)?,
                        revision: row.get::<_, i64>(1)? as u64,
                        expires_at: row.get(2)?,
                    })
                },
            )
            .optional()?;
        let found = found.filter(|stored| stored.expires_at.is_none_or(|at| at > now));
        if found.is_some() {
            self.conn.execute(
                "UPDATE entries SET last_used_at = ?2 WHERE key = ?1",
                params![key, now],
            )?;
        }
        Ok(found)
    }

    /// Lists persistent logical entries in key order without loading their values.
    pub fn persistence_entries(
        &self,
        now: i64,
    ) -> Result<Vec<PersistenceEntry>, PluginStorageError> {
        let mut statement = self.conn.prepare(
            "SELECT key, length(value), last_used_at, user_protected FROM entries
             WHERE archived = 0 AND (expires_at IS NULL OR expires_at > ?1) ORDER BY key",
        )?;
        let rows = statement.query_map([now], |row| {
            Ok(PersistenceEntry {
                key: row.get(0)?,
                size_bytes: row.get::<_, i64>(1)? as u64,
                last_used_at: row.get(2)?,
                user_protected: row.get::<_, i64>(3)? != 0,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// The retained limits for one logical category.
    pub fn retention_policy(
        &self,
        category: &str,
    ) -> Result<Option<RetentionPolicy>, PluginStorageError> {
        self.conn
            .query_row(
                "SELECT max_age_seconds, max_bytes FROM retention_policies WHERE category = ?1",
                [category],
                |row| {
                    Ok(RetentionPolicy {
                        max_age_seconds: row.get::<_, i64>(0)? as u64,
                        max_bytes: row.get::<_, i64>(1)? as u64,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// Updates one logical category's retention limits and immediately removes entries that
    /// exceed the newly selected age/size bounds. `revision` is the namespace high-water mark.
    pub fn set_retention_policy(
        &mut self,
        category: &str,
        policy: RetentionPolicy,
        now: i64,
        revision: u64,
    ) -> Result<u64, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO retention_policies (category, max_age_seconds, max_bytes)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(category) DO UPDATE SET
                 max_age_seconds = excluded.max_age_seconds,
                 max_bytes = excluded.max_bytes",
            params![
                category,
                policy.max_age_seconds as i64,
                policy.max_bytes as i64
            ],
        )?;
        let reclaimed = prune_to_policy(&tx, category, policy, now, None)?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(reclaimed)
    }

    /// Deletes selected logical keys in one transaction and advances the namespace revision.
    /// A user-protected key refuses the set, so a plugin cannot delete protected data by naming
    /// it in a bulk deletion.
    pub fn delete_persistence_entries(
        &mut self,
        keys: &[String],
        now: i64,
        revision: u64,
    ) -> Result<u64, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for key in keys {
            let protected: Option<i64> = tx
                .query_row(
                    "SELECT user_protected FROM entries WHERE key = ?1 AND archived = 0
                     AND (expires_at IS NULL OR expires_at > ?2)",
                    params![key, now],
                    |row| row.get(0),
                )
                .optional()?;
            if protected == Some(1) {
                return Err(PluginStorageError::Protected);
            }
        }
        let mut reclaimed = 0_u64;
        for key in keys {
            let size: Option<i64> = tx
                .query_row(
                    "SELECT length(value) FROM entries WHERE key = ?1 AND archived = 0
                     AND (expires_at IS NULL OR expires_at > ?2)",
                    params![key, now],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(size) = size {
                reclaimed = reclaimed.saturating_add(charge(key, size as usize));
                tx.execute("DELETE FROM entries WHERE key = ?1 AND archived = 0", [key])?;
            }
        }
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(reclaimed)
    }

    /// Approximate database page allocation without disclosing its path. Metadata and free pages
    /// are shared by every category in the same file.
    pub fn physical_bytes(&self) -> Result<u64, PluginStorageError> {
        let page_count: i64 = self
            .conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))?;
        let page_size: i64 = self
            .conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))?;
        Ok(page_count.max(0) as u64 * page_size.max(0) as u64)
    }

    /// The logical bytes held at `now`, expired entries not counted.
    pub fn usage(&self, now: i64) -> Result<u64, PluginStorageError> {
        usage_of(&self.conn, now, None)
    }

    /// Per-category logical accounting plus the namespace-wide physical estimate. Values and
    /// paths never leave the store.
    pub fn store_usage(&self, now: i64) -> Result<StoreUsage, PluginStorageError> {
        let (records, logical_bytes, protected_records, oldest_used_at): (
            i64,
            i64,
            i64,
            Option<i64>,
        ) = self.conn.query_row(
            "SELECT COUNT(*),
                        COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0),
                        COALESCE(SUM(user_protected), 0),
                        MIN(last_used_at)
                 FROM entries
                 WHERE archived = 0 AND (expires_at IS NULL OR expires_at > ?2)",
            params![ENTRY_OVERHEAD_BYTES as i64, now],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let (archived_records, archived_logical_bytes): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
             FROM entries WHERE archived = 1",
            [ENTRY_OVERHEAD_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let category = CategoryUsage {
            category: "private".to_owned(),
            records: records.max(0) as u64,
            logical_bytes: logical_bytes.max(0) as u64,
            protected_records: protected_records.max(0) as u64,
        };
        Ok(StoreUsage {
            logical_bytes: category.logical_bytes,
            categories: vec![category],
            physical_bytes_estimate: self.physical_bytes()?,
            archived_records: archived_records.max(0) as u64,
            archived_logical_bytes: archived_logical_bytes.max(0) as u64,
            oldest_used_at,
            retention: self.retention_policy("private")?,
        })
    }

    /// Applies the host-only user protection decision. A plugin RPC has no way to reach this.
    pub fn set_user_protected(
        &mut self,
        key: &str,
        protected: bool,
    ) -> Result<bool, PluginStorageError> {
        let changed = self.conn.execute(
            "UPDATE entries SET user_protected = ?2 WHERE key = ?1 AND archived = 0",
            params![key, i64::from(protected as u8)],
        )?;
        Ok(changed != 0)
    }

    /// Makes every live entry of this namespace inert for uninstall Keep. The rows stay on disk
    /// with their revisions; only the host reviewed reuse restores them.
    pub fn archive_entries(
        &mut self,
        revision: u64,
    ) -> Result<PrivateArchiveOutcome, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let archived = tx.execute("UPDATE entries SET archived = 1 WHERE archived = 0", [])? as u64;
        let (_, logical_bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
             FROM entries WHERE archived = 1",
            [ENTRY_OVERHEAD_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(PrivateArchiveOutcome {
            records: archived,
            logical_bytes: logical_bytes.max(0) as u64,
        })
    }

    /// Restores this owner's archived entries after an explicit host review. Expired archived
    /// rows are discarded instead of being restored as visible-but-expired entries.
    pub fn reuse_archived_entries(
        &mut self,
        now: i64,
        revision: u64,
    ) -> Result<PrivateArchiveOutcome, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM entries WHERE archived = 1
               AND expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        let restored = tx.execute("UPDATE entries SET archived = 0 WHERE archived = 1", [])? as u64;
        let (_, logical_bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
             FROM entries WHERE archived = 0
               AND (expires_at IS NULL OR expires_at > ?2)",
            params![ENTRY_OVERHEAD_BYTES as i64, now],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(PrivateArchiveOutcome {
            records: restored,
            logical_bytes: logical_bytes.max(0) as u64,
        })
    }

    /// The archived entries of this owner, without their values or a path. Used by the host
    /// review of retained data; a plugin RPC cannot reach it.
    pub fn archived_usage(&self) -> Result<PrivateArchiveOutcome, PluginStorageError> {
        let (records, logical_bytes): (i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
             FROM entries WHERE archived = 1",
            [ENTRY_OVERHEAD_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(PrivateArchiveOutcome {
            records: records.max(0) as u64,
            logical_bytes: logical_bytes.max(0) as u64,
        })
    }

    /// Reclaims this owner's archived, unprotected entries in a reviewed host cleanup. A
    /// protected archived entry stays until the user lifts its protection. Advances the
    /// namespace revision so cursors cannot resume across the change.
    pub fn reclaim_archived(
        &mut self,
        revision: u64,
    ) -> Result<PrivateArchiveOutcome, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (records, logical_bytes): (i64, i64) = tx.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
             FROM entries WHERE archived = 1 AND user_protected = 0",
            [ENTRY_OVERHEAD_BYTES as i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        tx.execute(
            "DELETE FROM entries WHERE archived = 1 AND user_protected = 0",
            [],
        )?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(PrivateArchiveOutcome {
            records: records.max(0) as u64,
            logical_bytes: logical_bytes.max(0) as u64,
        })
    }

    /// Removes expired records and applies the stored retention policy while keeping every
    /// user-protected record. This is a host cleanup, separate from a plugin mutation.
    pub fn cleanup(&mut self, now: i64) -> Result<CleanupOutcome, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let before_records: i64 = tx.query_row(
            "SELECT COUNT(*) FROM entries WHERE archived = 0",
            [],
            |row| row.get(0),
        )?;
        let before_bytes = usage_of(&tx, i64::MIN, None)?;
        tx.execute(
            "DELETE FROM entries WHERE archived = 0 AND user_protected = 0
               AND expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        let policy = tx
            .query_row(
                "SELECT max_age_seconds, max_bytes FROM retention_policies WHERE category = 'private'",
                [],
                |row| {
                    Ok(RetentionPolicy {
                        max_age_seconds: row.get::<_, i64>(0)? as u64,
                        max_bytes: row.get::<_, i64>(1)? as u64,
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        prune_to_policy(&tx, "private", policy, now, None)?;
        let after_records: i64 = tx.query_row(
            "SELECT COUNT(*) FROM entries WHERE archived = 0",
            [],
            |row| row.get(0),
        )?;
        let after_bytes = usage_of(&tx, i64::MIN, None)?;
        tx.commit()?;
        Ok(CleanupOutcome {
            removed_records: before_records.saturating_sub(after_records).max(0) as u64,
            reclaimed_bytes: before_bytes.saturating_sub(after_bytes),
        })
    }

    /// Stores `value` under `key` with `revision`, replacing any entry, unless the namespace
    /// would hold more than `quota` bytes afterwards. Expired entries are dropped in the same
    /// transaction. The caller has already checked the expected revision.
    pub fn put(
        &mut self,
        key: &str,
        value: &[u8],
        revision: u64,
        expires_at: Option<i64>,
        now: i64,
        quota: u64,
    ) -> Result<PutResult, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "DELETE FROM entries WHERE archived = 0 AND user_protected = 0
               AND expires_at IS NOT NULL AND expires_at <= ?1",
            [now],
        )?;
        let archived: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM entries WHERE key = ?1 AND archived = 1)",
            [key],
            |row| row.get(0),
        )?;
        if archived {
            return Err(PluginStorageError::Archived);
        }
        let protected: Option<i64> = tx
            .query_row(
                "SELECT user_protected FROM entries WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        if protected == Some(1) {
            return Err(PluginStorageError::Protected);
        }
        let policy = tx
            .query_row(
                "SELECT max_age_seconds, max_bytes FROM retention_policies WHERE category = 'private'",
                [],
                |row| {
                    Ok(RetentionPolicy {
                        max_age_seconds: row.get::<_, i64>(0)? as u64,
                        max_bytes: row.get::<_, i64>(1)? as u64,
                    })
                },
            )
            .optional()?
            .unwrap_or_default();
        prune_to_policy(&tx, "private", policy, now, Some(key))?;
        let others = usage_of(&tx, now, Some(key))?;
        let current = usage_of(&tx, now, None)?;
        if others.saturating_add(charge(key, value.len())) > quota {
            return Ok(PutResult::OverQuota { usage: current });
        }
        tx.execute(
            "INSERT INTO entries (key, value, revision, expires_at, last_used_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 revision = excluded.revision,
                 expires_at = excluded.expires_at,
                 last_used_at = excluded.last_used_at",
            params![key, value, revision as i64, expires_at, now],
        )?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(PutResult::Stored {
            usage: others + charge(key, value.len()),
        })
    }

    /// Deletes `key`. Returns the revision it had, or `None` when there was no such entry. A
    /// user-protected key is refused.
    pub fn remove(&mut self, key: &str) -> Result<Option<u64>, PluginStorageError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let entry: Option<(i64, i64)> = tx
            .query_row(
                "SELECT revision, user_protected FROM entries WHERE key = ?1 AND archived = 0",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((revision, protected)) = entry else {
            return Ok(None);
        };
        if protected != 0 {
            return Err(PluginStorageError::Protected);
        }
        tx.execute("DELETE FROM entries WHERE key = ?1 AND archived = 0", [key])?;
        tx.commit()?;
        Ok(Some(revision as u64))
    }
}

fn prune_to_policy(
    tx: &rusqlite::Transaction<'_>,
    category: &str,
    policy: RetentionPolicy,
    now: i64,
    keep_key: Option<&str>,
) -> Result<u64, PluginStorageError> {
    if category != "private" {
        return Ok(0);
    }
    let mut reclaimed = 0_u64;
    if policy.max_age_seconds > 0 {
        let cutoff = now.saturating_sub(policy.max_age_seconds.min(i64::MAX as u64) as i64);
        let mut statement = tx.prepare(
            "SELECT key, length(value) FROM entries WHERE archived = 0 AND user_protected = 0
             AND last_used_at <= ?1 AND (?2 IS NULL OR key != ?2)",
        )?;
        let old = statement
            .query_map(params![cutoff, keep_key], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);
        for (key, size) in old {
            tx.execute(
                "DELETE FROM entries WHERE key = ?1 AND archived = 0",
                [&key],
            )?;
            reclaimed = reclaimed.saturating_add(charge(&key, size));
        }
    }
    if policy.max_bytes > 0 {
        let mut usage = usage_of(tx, now, None)?;
        while usage > policy.max_bytes {
            let oldest = tx
                .query_row(
                    "SELECT key, length(value) FROM entries
                     WHERE archived = 0 AND user_protected = 0
                       AND (?1 IS NULL OR key != ?1)
                       AND (expires_at IS NULL OR expires_at > ?2)
                     ORDER BY last_used_at ASC, key ASC LIMIT 1",
                    params![keep_key, now],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize)),
                )
                .optional()?;
            let Some((key, size)) = oldest else {
                break;
            };
            let bytes = charge(&key, size);
            tx.execute(
                "DELETE FROM entries WHERE key = ?1 AND archived = 0",
                [&key],
            )?;
            usage = usage.saturating_sub(bytes);
            reclaimed = reclaimed.saturating_add(bytes);
        }
    }
    Ok(reclaimed)
}

/// The file operations a root approval allows.
pub mod operation {
    pub const READ: u8 = 1;
    pub const WRITE: u8 = 2;
    pub const DELETE: u8 = 4;
}

/// A folder the user approved for one plugin and one installation of a game.
///
/// It is kept per plugin and account profile, apart from the plugin's key/value data and not
/// charged to its quota. The plugin never sees `root`; it only receives handles issued for the
/// approval while it is current.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileApproval {
    /// Opaque random id; a new approval of the same installation and kind gets a new one.
    pub id: String,
    pub installation_id: String,
    /// The GameNode game the installation was matched to when approved (empty if none).
    pub game_id: String,
    /// What the folder is: `installation`, `save` or `prefix`.
    pub kind: String,
    /// The canonical location, valid Unicode.
    pub root: String,
    pub volume_token: String,
    pub root_volume: u64,
    pub root_node: u64,
    /// A combination of [`operation`] bits.
    pub operations: u8,
    /// The library entries that overlapped the folder when the user approved it, sorted. The
    /// approval holds only while this is still the complete set.
    pub affected: Vec<String>,
    /// Unix seconds.
    pub approved_at: i64,
    /// Unix seconds; a revoked approval opens nothing.
    pub revoked_at: Option<i64>,
    /// How the folder was chosen: [`ORIGIN_USER`] or [`ORIGIN_DECLARED`].
    pub origin: String,
}

/// The user picked the folder, or took the one the host suggested.
pub const ORIGIN_USER: &str = "user";
/// The user took a folder the plugin had suggested (declared, not verified by the host).
pub const ORIGIN_DECLARED: &str = "declared";

fn approval_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileApproval> {
    let affected: String = row.get(8)?;
    Ok(FileApproval {
        id: row.get(0)?,
        installation_id: row.get(1)?,
        game_id: row.get(2)?,
        kind: row.get(3)?,
        root: row.get(4)?,
        root_volume: row.get::<_, i64>(5)? as u64,
        root_node: row.get::<_, i64>(6)? as u64,
        operations: row.get::<_, i64>(7)? as u8,
        affected: serde_json::from_str(&affected).unwrap_or_default(),
        approved_at: row.get(9)?,
        revoked_at: row.get(10)?,
        origin: row.get(11)?,
        volume_token: row.get(12)?,
    })
}

/// Where the user's chosen executable is and what it was when approved.
///
/// It is kept per plugin and account profile, apart from the plugin's key/value data and not
/// charged to its quota. The plugin never sees `path`: the runtime record points at this approval,
/// and the host resolves the path only after checking the identity and digest before a launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeApproval {
    /// Opaque random id in the runtime's durable file reference; a reapproval gets a new one.
    pub id: String,
    /// The owned runtime this approval belongs to.
    pub runtime_id: String,
    /// The canonical executable location, valid Unicode.
    pub path: String,
    /// The volume and node identity of the approved file.
    pub volume: u64,
    pub node: u64,
    pub size: u64,
    /// Lowercase hexadecimal SHA-256 of the approved file.
    pub digest: String,
    /// Unix seconds.
    pub approved_at: i64,
    /// Unix seconds; a revoked approval launches nothing.
    pub revoked_at: Option<i64>,
}

fn runtime_approval_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<RuntimeApproval> {
    Ok(RuntimeApproval {
        id: row.get(0)?,
        runtime_id: row.get(1)?,
        path: row.get(2)?,
        volume: row.get::<_, i64>(3)? as u64,
        node: row.get::<_, i64>(4)? as u64,
        size: row.get::<_, i64>(5)? as u64,
        digest: row.get(6)?,
        approved_at: row.get(7)?,
        revoked_at: row.get(8)?,
    })
}

const RUNTIME_APPROVAL_COLUMNS: &str =
    "id, runtime_id, path, volume, node, size, digest, approved_at, revoked_at";

impl DurableStore {
    /// Records `approval`, replacing the earlier approval of the same runtime.
    pub fn save_runtime_approval(
        &mut self,
        approval: &RuntimeApproval,
    ) -> Result<(), PluginStorageError> {
        self.conn.execute(
            "INSERT INTO runtime_approvals
                 (id, runtime_id, path, volume, node, size, digest, approved_at, revoked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(runtime_id) DO UPDATE SET
                 id = excluded.id,
                 path = excluded.path,
                 volume = excluded.volume,
                 node = excluded.node,
                 size = excluded.size,
                 digest = excluded.digest,
                 approved_at = excluded.approved_at,
                 revoked_at = excluded.revoked_at",
            rusqlite::params![
                approval.id,
                approval.runtime_id,
                approval.path,
                approval.volume as i64,
                approval.node as i64,
                approval.size as i64,
                approval.digest,
                approval.approved_at,
                approval.revoked_at,
            ],
        )?;
        Ok(())
    }

    /// The approval for `runtime_id`, revoked or active.
    pub fn runtime_approval_for(
        &self,
        runtime_id: &str,
    ) -> Result<Option<RuntimeApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {RUNTIME_APPROVAL_COLUMNS} FROM runtime_approvals
                     WHERE runtime_id = ?1"
                ),
                [runtime_id],
                runtime_approval_from,
            )
            .optional()?)
    }

    /// The approval with this id, revoked or active.
    pub fn runtime_approval_by_id(
        &self,
        id: &str,
    ) -> Result<Option<RuntimeApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {RUNTIME_APPROVAL_COLUMNS} FROM runtime_approvals
                     WHERE id = ?1"
                ),
                [id],
                runtime_approval_from,
            )
            .optional()?)
    }

    /// Every runtime approval in this namespace, revoked or active.
    pub fn runtime_approvals(&self) -> Result<Vec<RuntimeApproval>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {RUNTIME_APPROVAL_COLUMNS} FROM runtime_approvals ORDER BY approved_at, id"
        ))?;
        let rows = statement.query_map([], runtime_approval_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Revokes the approval of `runtime_id`; a missing or already revoked approval is not an error.
    pub fn revoke_runtime_approval(
        &mut self,
        runtime_id: &str,
        at: i64,
    ) -> Result<bool, PluginStorageError> {
        let changed = self.conn.execute(
            "UPDATE runtime_approvals SET revoked_at = ?2
             WHERE runtime_id = ?1 AND revoked_at IS NULL",
            rusqlite::params![runtime_id, at],
        )?;
        Ok(changed > 0)
    }
}

const APPROVAL_COLUMNS: &str = "id, installation_id, game_id, kind, root, root_volume, root_node, \
     operations, affected, approved_at, revoked_at, origin, volume_token";

impl DurableStore {
    /// Records `approval`, replacing the earlier approval of the same installation and kind.
    pub fn save_approval(&mut self, approval: &FileApproval) -> Result<(), PluginStorageError> {
        let affected = serde_json::to_string(&approval.affected).unwrap_or_else(|_| "[]".into());
        self.conn.execute(
            "INSERT INTO file_roots (id, installation_id, game_id, kind, root, root_volume,
                 root_node, operations, affected, approved_at, revoked_at, origin, volume_token)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(installation_id, kind) DO UPDATE SET
                 id = excluded.id, game_id = excluded.game_id, root = excluded.root,
                 root_volume = excluded.root_volume, root_node = excluded.root_node,
                 operations = excluded.operations, affected = excluded.affected,
                 approved_at = excluded.approved_at, revoked_at = excluded.revoked_at,
                 origin = excluded.origin, volume_token = excluded.volume_token",
            params![
                approval.id,
                approval.installation_id,
                approval.game_id,
                approval.kind,
                approval.root,
                approval.root_volume as i64,
                approval.root_node as i64,
                i64::from(approval.operations),
                affected,
                approval.approved_at,
                approval.revoked_at,
                approval.origin,
                approval.volume_token,
            ],
        )?;
        Ok(())
    }

    /// The approval of `installation_id` for `kind`, revoked or not.
    pub fn approval_for(
        &self,
        installation_id: &str,
        kind: &str,
    ) -> Result<Option<FileApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {APPROVAL_COLUMNS} FROM file_roots
                     WHERE installation_id = ?1 AND kind = ?2"
                ),
                params![installation_id, kind],
                approval_from,
            )
            .optional()?)
    }

    /// The approval with this id, revoked or not.
    pub fn approval(&self, id: &str) -> Result<Option<FileApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {APPROVAL_COLUMNS} FROM file_roots WHERE id = ?1"),
                [id],
                approval_from,
            )
            .optional()?)
    }

    /// Every approval, in a stable order.
    pub fn approvals(&self) -> Result<Vec<FileApproval>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {APPROVAL_COLUMNS} FROM file_roots ORDER BY installation_id, kind"
        ))?;
        let rows = statement.query_map([], approval_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Revokes an approval; its handles stop working at once. Returns whether it was active.
    pub fn revoke_approval(&mut self, id: &str, now: i64) -> Result<bool, PluginStorageError> {
        let changed = self.conn.execute(
            "UPDATE file_roots SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL",
            params![id, now],
        )?;
        Ok(changed > 0)
    }
}

/// A confirmation the user asked the host to remember: this plugin may launch (or install) this
/// game without asking again, while the game's detected target is the one approved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentApproval {
    /// Opaque random id the settings list uses to revoke it.
    pub id: String,
    pub game_id: String,
    pub installation_id: String,
    /// `launch`, `custom_launch`, or `install`.
    pub operation: String,
    /// The detected-target revision approved (0 for an install, which has none).
    pub target_revision: u64,
    /// The custom recipe id approved (empty for normal launch and install).
    pub recipe_id: String,
    /// The custom recipe revision approved (0 for normal launch and install).
    pub recipe_revision: u64,
    /// Canonical launch-plan approval fingerprint; empty on legacy approvals that must reconfirm.
    pub plan_fingerprint: String,
    /// The game's name when approved, for the list; plain text.
    pub game_name: String,
    pub approved_at: i64,
}

const INTENT_COLUMNS: &str = "id, game_id, installation_id, operation, target_revision, recipe_id, recipe_revision, game_name, approved_at, plan_fingerprint";

fn intent_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<IntentApproval> {
    Ok(IntentApproval {
        id: row.get(0)?,
        game_id: row.get(1)?,
        installation_id: row.get(2)?,
        operation: row.get(3)?,
        target_revision: row.get::<_, i64>(4)? as u64,
        recipe_id: row.get(5)?,
        recipe_revision: row.get::<_, i64>(6)? as u64,
        game_name: row.get(7)?,
        approved_at: row.get(8)?,
        plan_fingerprint: row.get(9)?,
    })
}

impl DurableStore {
    /// Remembers `approval`, replacing the earlier one for the same installation and operation.
    pub fn save_intent_approval(
        &mut self,
        approval: &IntentApproval,
    ) -> Result<(), PluginStorageError> {
        self.conn.execute(
            "INSERT INTO intent_approvals (id, game_id, installation_id, operation,
                 target_revision, recipe_id, recipe_revision, game_name, approved_at, plan_fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(installation_id, operation) DO UPDATE SET
                 id = excluded.id, game_id = excluded.game_id,
                 target_revision = excluded.target_revision,
                 recipe_id = excluded.recipe_id,
                 recipe_revision = excluded.recipe_revision, game_name = excluded.game_name,
                 approved_at = excluded.approved_at, plan_fingerprint = excluded.plan_fingerprint",
            params![
                approval.id,
                approval.game_id,
                approval.installation_id,
                approval.operation,
                approval.target_revision as i64,
                approval.recipe_id,
                approval.recipe_revision as i64,
                approval.game_name,
                approval.approved_at,
                approval.plan_fingerprint,
            ],
        )?;
        Ok(())
    }

    /// The remembered approval of `installation_id` for `operation`.
    pub fn intent_approval(
        &self,
        installation_id: &str,
        operation: &str,
    ) -> Result<Option<IntentApproval>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!(
                    "SELECT {INTENT_COLUMNS} FROM intent_approvals
                     WHERE installation_id = ?1 AND operation = ?2"
                ),
                params![installation_id, operation],
                intent_from,
            )
            .optional()?)
    }

    /// Every remembered approval, in a stable order.
    pub fn intent_approvals(&self) -> Result<Vec<IntentApproval>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {INTENT_COLUMNS} FROM intent_approvals ORDER BY installation_id, operation"
        ))?;
        let rows = statement.query_map([], intent_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Forgets an approval; the next request asks the user again. Returns whether it existed.
    pub fn revoke_intent_approval(&mut self, id: &str) -> Result<bool, PluginStorageError> {
        let changed = self
            .conn
            .execute("DELETE FROM intent_approvals WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }
}

/// A save profile a plugin contributed, as stored. `directories` and `metadata` are JSON the host
/// wrote and validated; the store only keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveProfileRow {
    pub id: String,
    pub installation_id: String,
    /// The GameNode game the installation was matched to when created (empty if none). A later
    /// rematch does not change it.
    pub game_id: String,
    pub name: String,
    pub runtime_id: String,
    pub format_id: String,
    pub format_version: u32,
    pub metadata: String,
    pub directories: String,
    /// Host-resolved shared emulator resource declarations, as JSON. The store only keeps them.
    pub shared_resources: String,
    pub revision: u64,
    pub created_at: i64,
}

/// How writing a profile ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSaved {
    Saved,
    /// The installation already has a profile with this name (compared without case).
    NameTaken,
}

const PROFILE_COLUMNS: &str = "id, installation_id, game_id, name, runtime_id, format_id, \
     format_version, metadata, directories, shared_resources, revision, created_at";

fn profile_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<SaveProfileRow> {
    Ok(SaveProfileRow {
        id: row.get(0)?,
        installation_id: row.get(1)?,
        game_id: row.get(2)?,
        name: row.get(3)?,
        runtime_id: row.get(4)?,
        format_id: row.get(5)?,
        format_version: row.get::<_, i64>(6)? as u32,
        metadata: row.get(7)?,
        directories: row.get(8)?,
        shared_resources: row.get(9)?,
        revision: row.get::<_, i64>(10)? as u64,
        created_at: row.get(11)?,
    })
}

impl DurableStore {
    /// Inserts or replaces the profile with `row.id`, and raises the store's revision high-water
    /// mark to `row.revision` so no revision is ever handed out twice.
    pub fn save_profile(
        &mut self,
        row: &SaveProfileRow,
    ) -> Result<ProfileSaved, PluginStorageError> {
        let tx = self.conn.transaction()?;
        let name_key = row.name.to_lowercase();
        let written = tx.execute(
            "INSERT INTO save_profiles (id, installation_id, game_id, name, name_key, runtime_id,
                 format_id, format_version, metadata, directories, shared_resources, revision,
                 created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name, name_key = excluded.name_key,
                 runtime_id = excluded.runtime_id, format_id = excluded.format_id,
                 format_version = excluded.format_version, metadata = excluded.metadata,
                 directories = excluded.directories,
                 shared_resources = excluded.shared_resources, revision = excluded.revision",
            params![
                row.id,
                row.installation_id,
                row.game_id,
                row.name,
                name_key,
                row.runtime_id,
                row.format_id,
                i64::from(row.format_version),
                row.metadata,
                row.directories,
                row.shared_resources,
                row.revision as i64,
                row.created_at,
            ],
        );
        match written {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                return Ok(ProfileSaved::NameTaken);
            }
            Err(other) => return Err(other.into()),
        }
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [row.revision as i64],
        )?;
        tx.commit()?;
        Ok(ProfileSaved::Saved)
    }

    /// The profile with this id.
    pub fn save_profile_row(&self, id: &str) -> Result<Option<SaveProfileRow>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {PROFILE_COLUMNS} FROM save_profiles WHERE id = ?1"),
                [id],
                profile_from,
            )
            .optional()?)
    }

    /// Every profile, ordered by installation and id.
    pub fn save_profile_rows(&self) -> Result<Vec<SaveProfileRow>, PluginStorageError> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {PROFILE_COLUMNS} FROM save_profiles ORDER BY installation_id, id"
        ))?;
        let rows = statement.query_map([], profile_from)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Removes the profile, raising the revision high-water mark to `revision` (the removal's own
    /// revision). Returns whether it existed.
    pub fn delete_save_profile(
        &mut self,
        id: &str,
        revision: u64,
    ) -> Result<bool, PluginStorageError> {
        let tx = self.conn.transaction()?;
        let removed = tx.execute("DELETE FROM save_profiles WHERE id = ?1", [id])?;
        tx.execute(
            "DELETE FROM shared_resource_consents WHERE profile_id = ?1",
            [id],
        )?;
        tx.execute(
            "UPDATE meta SET revision = MAX(revision, ?1) WHERE id = 1",
            [revision as i64],
        )?;
        tx.commit()?;
        Ok(removed > 0)
    }
}

/// A persisted user decision about one declared shared resource. `declaration` and `scope` are
/// canonical host fingerprints, never plugin-supplied text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedResourceConsentRow {
    pub profile_id: String,
    pub resource_id: String,
    pub declaration: String,
    pub scope: String,
    pub profile_revision: u64,
    pub approved_at: i64,
}

impl DurableStore {
    /// Records or replaces the user's decision for one declaration.
    pub fn save_shared_resource_consent(
        &mut self,
        row: &SharedResourceConsentRow,
    ) -> Result<(), PluginStorageError> {
        self.conn.execute(
            "INSERT INTO shared_resource_consents
                 (profile_id, resource_id, declaration, scope, profile_revision, approved_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(profile_id, resource_id) DO UPDATE SET
                 declaration = excluded.declaration, scope = excluded.scope,
                 profile_revision = excluded.profile_revision,
                 approved_at = excluded.approved_at",
            params![
                row.profile_id,
                row.resource_id,
                row.declaration,
                row.scope,
                row.profile_revision as i64,
                row.approved_at,
            ],
        )?;
        Ok(())
    }

    /// The decision for one declaration, if the user made one.
    pub fn shared_resource_consent(
        &self,
        profile_id: &str,
        resource_id: &str,
    ) -> Result<Option<SharedResourceConsentRow>, PluginStorageError> {
        Ok(self
            .conn
            .query_row(
                "SELECT profile_id, resource_id, declaration, scope, profile_revision, approved_at
                 FROM shared_resource_consents WHERE profile_id = ?1 AND resource_id = ?2",
                params![profile_id, resource_id],
                |row| {
                    Ok(SharedResourceConsentRow {
                        profile_id: row.get(0)?,
                        resource_id: row.get(1)?,
                        declaration: row.get(2)?,
                        scope: row.get(3)?,
                        profile_revision: row.get::<_, i64>(4)? as u64,
                        approved_at: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    /// Every decision kept for a profile.
    pub fn shared_resource_consents(
        &self,
        profile_id: &str,
    ) -> Result<Vec<SharedResourceConsentRow>, PluginStorageError> {
        let mut statement = self.conn.prepare(
            "SELECT profile_id, resource_id, declaration, scope, profile_revision, approved_at
             FROM shared_resource_consents WHERE profile_id = ?1 ORDER BY resource_id",
        )?;
        let rows = statement.query_map([profile_id], |row| {
            Ok(SharedResourceConsentRow {
                profile_id: row.get(0)?,
                resource_id: row.get(1)?,
                declaration: row.get(2)?,
                scope: row.get(3)?,
                profile_revision: row.get::<_, i64>(4)? as u64,
                approved_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Forgets the decision, so the next use asks the user again.
    pub fn revoke_shared_resource_consent(
        &mut self,
        profile_id: &str,
        resource_id: &str,
    ) -> Result<bool, PluginStorageError> {
        Ok(self.conn.execute(
            "DELETE FROM shared_resource_consents WHERE profile_id = ?1 AND resource_id = ?2",
            params![profile_id, resource_id],
        )? > 0)
    }
}

/// The charged bytes of the live entries, optionally leaving out one key.
fn usage_of(conn: &Connection, now: i64, except: Option<&str>) -> Result<u64, PluginStorageError> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(length(CAST(key AS BLOB)) + length(value) + ?1), 0)
         FROM entries
         WHERE archived = 0
           AND (expires_at IS NULL OR expires_at > ?2) AND (?3 IS NULL OR key != ?3)",
        params![ENTRY_OVERHEAD_BYTES as i64, now, except],
        |row| row.get(0),
    )?;
    Ok(total as u64)
}

#[cfg(test)]
mod tests;

pub const MAX_PLUGIN_LIBRARIES: usize = 128;
pub const MAX_PLUGIN_LIBRARY_ROOTS: usize = 128;

/// Host-owned logical library data. Kept outside the plugin's private key/value API.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginLibraryRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: u64,
    pub removed: bool,
    pub roots: Vec<PluginLibraryRoot>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PluginLibraryRoot {
    pub id: String,
    pub name: String,
    pub path: String,
    pub volume: u64,
    pub node: u64,
    pub volume_token: String,
    pub declared: bool,
    pub watching: bool,
}

impl DurableStore {
    pub fn plugin_libraries(&self) -> Result<Vec<PluginLibraryRow>, PluginStorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT payload FROM plugin_libraries ORDER BY id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| {
            let text = row?;
            serde_json::from_str(&text).map_err(|error| {
                PluginStorageError::Database(rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                ))
            })
        })
        .collect()
    }

    pub fn put_plugin_library(&mut self, row: &PluginLibraryRow) -> Result<(), PluginStorageError> {
        let payload = serde_json::to_string(row).expect("library records serialize");
        let tx = self.conn.transaction()?;
        tx.execute("INSERT INTO plugin_libraries(id,payload) VALUES (?1,?2) ON CONFLICT(id) DO UPDATE SET payload=excluded.payload", params![row.id,payload])?;
        tx.execute(
            "UPDATE meta SET revision=MAX(revision,?1) WHERE id=1",
            [row.revision as i64],
        )?;
        tx.commit()?;
        Ok(())
    }
}

impl PluginLibraryRoot {
    /// Session handles bind to the approved folder revision, not just the logical root id.
    pub fn handle_id(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for part in [
            &self.path,
            &self.volume_token,
            &self.volume.to_string(),
            &self.node.to_string(),
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        format!("{}:{:x}", self.id, hash.finalize())
    }
}

// Backup archives are host-owned game data, separate from plugin key/value quotas and cleanup.
impl DurableStore {
    pub fn backup_sources(&self, profile: &str) -> Result<Vec<String>, PluginStorageError> {
        self.conn
            .prepare(
                "SELECT destination FROM backup_sources WHERE profile_id=?1 ORDER BY destination",
            )?
            .query_map([profile], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }
    pub fn remember_backup_source(
        &self,
        profile: &str,
        destination: &str,
    ) -> Result<(), PluginStorageError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO backup_sources(profile_id,destination) VALUES (?1,?2)",
            params![profile, destination],
        )?;
        Ok(())
    }
}
