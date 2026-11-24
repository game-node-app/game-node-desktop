//! SQLite mapping between installed plugin bundles and their component packages.
//!
//! A bundle only groups packages that keep their own identity, grants and lifecycle. Component
//! packages, activation and payloads stay in the package registry and their existing stores; this
//! module remembers which installed plugin ids came from which bundle. Every write is one
//! transaction, and all methods perform blocking SQLite work that belongs on a worker thread.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use gamenode_core::ports::StorageError;
use rusqlite::{Connection, OptionalExtension, params};

/// One component of an installed bundle.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleComponentRecord {
    /// The component's own plugin id; it remains a normal installed package.
    pub plugin_id: String,
    /// `provider` or `extension`, exactly as declared by the component package.
    pub kind: String,
    /// Lowercase SHA-256 of the component archive declared by `bundle.json`.
    pub component_sha256: String,
    /// The component package version, independent of the bundle version.
    pub package_version: String,
}

/// An installed bundle and every component it maps to.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleRecord {
    pub bundle_id: String,
    pub name: String,
    pub version: String,
    pub publisher_fingerprint: String,
    /// Lowercase SHA-256 of the exact signed `bundle.json` bytes.
    pub manifest_sha256: String,
    /// Unix seconds when the mapping was recorded.
    pub installed_at: i64,
    pub components: Vec<BundleComponentRecord>,
}

/// The bundle mapping on the shared application database connection. Cloning shares that one
/// connection, so every handle observes the same rows.
#[derive(Clone)]
pub struct SqlitePluginBundleStore {
    conn: Arc<Mutex<Connection>>,
}

fn failed(error: impl std::fmt::Display) -> StorageError {
    StorageError::Failed(error.to_string())
}

fn invalid(message: &str) -> StorageError {
    StorageError::Failed(message.to_owned())
}

fn validate_bundle(bundle: &BundleRecord) -> Result<(), StorageError> {
    if [
        bundle.bundle_id.as_str(),
        bundle.name.as_str(),
        bundle.version.as_str(),
        bundle.publisher_fingerprint.as_str(),
        bundle.manifest_sha256.as_str(),
    ]
    .iter()
    .any(|value| value.is_empty())
    {
        return Err(invalid("a bundle record has an empty identity field"));
    }
    if bundle.installed_at < 0 {
        return Err(invalid("a bundle record has a negative install time"));
    }
    if bundle.components.is_empty() {
        return Err(invalid("a bundle record has no components"));
    }
    let mut plugin_ids = BTreeSet::new();
    for component in &bundle.components {
        if component.plugin_id.is_empty()
            || component.kind.is_empty()
            || component.component_sha256.is_empty()
            || component.package_version.is_empty()
        {
            return Err(invalid("a bundle component has an empty identity field"));
        }
        if !matches!(component.kind.as_str(), "provider" | "extension") {
            return Err(invalid(
                "a bundle component kind is not provider or extension",
            ));
        }
        if !plugin_ids.insert(component.plugin_id.as_str()) {
            return Err(invalid("a bundle record repeats a component plugin id"));
        }
    }
    Ok(())
}

fn read(conn: &Connection, bundle_id: &str) -> Result<Option<BundleRecord>, StorageError> {
    let bundle = conn
        .query_row(
            "SELECT bundle_id, name, version, publisher_fingerprint, manifest_sha256, installed_at
             FROM plugin_bundles WHERE bundle_id = ?1",
            [bundle_id],
            |row| {
                Ok(BundleRecord {
                    bundle_id: row.get(0)?,
                    name: row.get(1)?,
                    version: row.get(2)?,
                    publisher_fingerprint: row.get(3)?,
                    manifest_sha256: row.get(4)?,
                    installed_at: row.get(5)?,
                    components: Vec::new(),
                })
            },
        )
        .optional()
        .map_err(failed)?;
    let Some(mut bundle) = bundle else {
        return Ok(None);
    };
    let mut statement = conn
        .prepare(
            "SELECT plugin_id, kind, component_sha256, package_version
             FROM plugin_bundle_components WHERE bundle_id = ?1 ORDER BY plugin_id",
        )
        .map_err(failed)?;
    let rows = statement
        .query_map([bundle_id], |row| {
            Ok(BundleComponentRecord {
                plugin_id: row.get(0)?,
                kind: row.get(1)?,
                component_sha256: row.get(2)?,
                package_version: row.get(3)?,
            })
        })
        .map_err(failed)?;
    bundle.components = rows.collect::<Result<Vec<_>, _>>().map_err(failed)?;
    Ok(Some(bundle))
}

fn read_history(
    conn: &Connection,
    bundle_id: &str,
    version: &str,
) -> Result<Option<BundleRecord>, StorageError> {
    let bundle = conn
        .query_row(
            "SELECT bundle_id, name, version, publisher_fingerprint, manifest_sha256, installed_at
             FROM plugin_bundle_history WHERE bundle_id = ?1 AND version = ?2",
            params![bundle_id, version],
            |row| {
                Ok(BundleRecord {
                    bundle_id: row.get(0)?,
                    name: row.get(1)?,
                    version: row.get(2)?,
                    publisher_fingerprint: row.get(3)?,
                    manifest_sha256: row.get(4)?,
                    installed_at: row.get(5)?,
                    components: Vec::new(),
                })
            },
        )
        .optional()
        .map_err(failed)?;
    let Some(mut bundle) = bundle else {
        return Ok(None);
    };
    let mut statement = conn
        .prepare(
            "SELECT plugin_id, kind, component_sha256, package_version
             FROM plugin_bundle_history_components
             WHERE bundle_id = ?1 AND version = ?2
             ORDER BY plugin_id",
        )
        .map_err(failed)?;
    let rows = statement
        .query_map(params![bundle_id, version], |row| {
            Ok(BundleComponentRecord {
                plugin_id: row.get(0)?,
                kind: row.get(1)?,
                component_sha256: row.get(2)?,
                package_version: row.get(3)?,
            })
        })
        .map_err(failed)?;
    bundle.components = rows.collect::<Result<Vec<_>, _>>().map_err(failed)?;
    Ok(Some(bundle))
}

/// Copies one mapping into history, replacing an older archived copy of the same version. The
/// current mapping is never modified here.
fn archive_bundle(
    conn: &Connection,
    bundle: &BundleRecord,
    archived_at: i64,
) -> Result<(), StorageError> {
    conn.execute(
        "INSERT OR REPLACE INTO plugin_bundle_history
            (bundle_id, version, name, publisher_fingerprint, manifest_sha256, installed_at, archived_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            bundle.bundle_id,
            bundle.version,
            bundle.name,
            bundle.publisher_fingerprint,
            bundle.manifest_sha256,
            bundle.installed_at,
            archived_at,
        ],
    )
    .map_err(failed)?;
    conn.execute(
        "DELETE FROM plugin_bundle_history_components WHERE bundle_id = ?1 AND version = ?2",
        params![bundle.bundle_id, bundle.version],
    )
    .map_err(failed)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO plugin_bundle_history_components
                (bundle_id, version, plugin_id, kind, component_sha256, package_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(failed)?;
    for component in &bundle.components {
        insert
            .execute(params![
                bundle.bundle_id,
                bundle.version,
                component.plugin_id,
                component.kind,
                component.component_sha256,
                component.package_version,
            ])
            .map_err(failed)?;
    }
    Ok(())
}

/// Replaces the current component rows of an existing bundle row. The bundle row itself is
/// updated in place so its identity stays the primary key.
fn replace_components(conn: &Connection, bundle: &BundleRecord) -> Result<(), StorageError> {
    conn.execute(
        "DELETE FROM plugin_bundle_components WHERE bundle_id = ?1",
        [&bundle.bundle_id],
    )
    .map_err(failed)?;
    let mut insert = conn
        .prepare(
            "INSERT INTO plugin_bundle_components
                (bundle_id, plugin_id, kind, component_sha256, package_version)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .map_err(failed)?;
    for component in &bundle.components {
        insert
            .execute(params![
                bundle.bundle_id,
                component.plugin_id,
                component.kind,
                component.component_sha256,
                component.package_version,
            ])
            .map_err(failed)?;
    }
    Ok(())
}

fn update_bundle_row(conn: &Connection, bundle: &BundleRecord) -> Result<(), StorageError> {
    let changed = conn
        .execute(
            "UPDATE plugin_bundles
             SET name = ?2, version = ?3, publisher_fingerprint = ?4, manifest_sha256 = ?5,
                 installed_at = ?6
             WHERE bundle_id = ?1",
            params![
                bundle.bundle_id,
                bundle.name,
                bundle.version,
                bundle.publisher_fingerprint,
                bundle.manifest_sha256,
                bundle.installed_at,
            ],
        )
        .map_err(failed)?;
    if changed != 1 {
        return Err(invalid("the bundle mapping no longer exists"));
    }
    Ok(())
}

impl SqlitePluginBundleStore {
    pub(crate) fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        crate::db::lock(&self.conn)
    }

    /// Records a bundle and all its components in one transaction. A duplicate bundle id, a
    /// component already mapped to another bundle, or an invalid record changes nothing.
    pub fn record_bundle(&self, bundle: &BundleRecord) -> Result<(), StorageError> {
        validate_bundle(bundle)?;
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        tx.execute(
            "INSERT INTO plugin_bundles
                (bundle_id, name, version, publisher_fingerprint, manifest_sha256, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                bundle.bundle_id,
                bundle.name,
                bundle.version,
                bundle.publisher_fingerprint,
                bundle.manifest_sha256,
                bundle.installed_at,
            ],
        )
        .map_err(failed)?;
        {
            let mut insert = tx
                .prepare(
                    "INSERT INTO plugin_bundle_components
                        (bundle_id, plugin_id, kind, component_sha256, package_version)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(failed)?;
            for component in &bundle.components {
                insert
                    .execute(params![
                        bundle.bundle_id,
                        component.plugin_id,
                        component.kind,
                        component.component_sha256,
                        component.package_version,
                    ])
                    .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    /// Replaces an installed bundle's version, manifest hash, install time and component rows in
    /// one transaction, archiving the previous mapping first. The previous record is returned so a
    /// caller that has just activated new component versions can restore them if a later step
    /// fails. A missing mapping and an unchanged version both change nothing.
    pub fn update_bundle(
        &self,
        bundle: &BundleRecord,
        archived_at: i64,
    ) -> Result<BundleRecord, StorageError> {
        validate_bundle(bundle)?;
        if archived_at < 0 {
            return Err(invalid("a bundle archive time is negative"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let previous = read(&tx, &bundle.bundle_id)?
            .ok_or_else(|| invalid("the bundle mapping no longer exists"))?;
        if previous.version == bundle.version
            && previous.manifest_sha256 == bundle.manifest_sha256
            && previous.components == bundle.components
        {
            return Err(invalid("the bundle update does not change the mapping"));
        }
        archive_bundle(&tx, &previous, archived_at)?;
        update_bundle_row(&tx, bundle)?;
        replace_components(&tx, bundle)?;
        tx.commit().map_err(failed)?;
        Ok(previous)
    }

    /// Versions of this bundle that are archived and can be restored, most recently archived
    /// first. The current version is never included.
    pub fn previous_versions(&self, bundle_id: &str) -> Result<Vec<String>, StorageError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare(
                "SELECT version FROM plugin_bundle_history
                 WHERE bundle_id = ?1 ORDER BY archived_at DESC, version DESC",
            )
            .map_err(failed)?;
        statement
            .query_map([bundle_id], |row| row.get::<_, String>(0))
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)
    }

    /// One archived mapping of this bundle, if it exists.
    pub fn previous_bundle(
        &self,
        bundle_id: &str,
        version: &str,
    ) -> Result<Option<BundleRecord>, StorageError> {
        read_history(&self.conn(), bundle_id, version)
    }

    /// Makes an archived version current again in one transaction: the current mapping is archived
    /// and the selected history entry is removed. Returns the restored record, or `None` when that
    /// version is not archived. Component packages, grants and activation are untouched here; the
    /// caller re-activates the restored versions and revalidates grants.
    pub fn restore_bundle_version(
        &self,
        bundle_id: &str,
        version: &str,
        archived_at: i64,
    ) -> Result<Option<BundleRecord>, StorageError> {
        if archived_at < 0 {
            return Err(invalid("a bundle archive time is negative"));
        }
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let current =
            read(&tx, bundle_id)?.ok_or_else(|| invalid("the bundle mapping no longer exists"))?;
        let Some(target) = read_history(&tx, bundle_id, version)? else {
            return Ok(None);
        };
        if target.version == current.version {
            return Err(invalid("the bundle already uses the requested version"));
        }
        archive_bundle(&tx, &current, archived_at)?;
        tx.execute(
            "DELETE FROM plugin_bundle_history_components WHERE bundle_id = ?1 AND version = ?2",
            params![bundle_id, version],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM plugin_bundle_history WHERE bundle_id = ?1 AND version = ?2",
            params![bundle_id, version],
        )
        .map_err(failed)?;
        update_bundle_row(&tx, &target)?;
        replace_components(&tx, &target)?;
        tx.commit().map_err(failed)?;
        Ok(Some(target))
    }

    /// The stored bundle and its components, with components ordered by plugin id.
    pub fn bundle(&self, bundle_id: &str) -> Result<Option<BundleRecord>, StorageError> {
        read(&self.conn(), bundle_id)
    }

    /// Every stored bundle and its components, ordered by bundle id. The inventory groups
    /// installed components by this mapping; a package without a row stays standalone.
    pub fn bundles(&self) -> Result<Vec<BundleRecord>, StorageError> {
        let conn = self.conn();
        let mut statement = conn
            .prepare("SELECT bundle_id FROM plugin_bundles ORDER BY bundle_id")
            .map_err(failed)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(failed)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(failed)?;
        let mut bundles = Vec::with_capacity(ids.len());
        for bundle_id in ids {
            if let Some(bundle) = read(&conn, &bundle_id)? {
                bundles.push(bundle);
            }
        }
        Ok(bundles)
    }

    /// The bundle that contains `plugin_id`, or `None` for an ungrouped standalone package.
    pub fn bundle_for_plugin(&self, plugin_id: &str) -> Result<Option<BundleRecord>, StorageError> {
        let conn = self.conn();
        let bundle_id: Option<String> = conn
            .query_row(
                "SELECT bundle_id FROM plugin_bundle_components WHERE plugin_id = ?1",
                [plugin_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        match bundle_id {
            Some(bundle_id) => read(&conn, &bundle_id),
            None => Ok(None),
        }
    }

    /// Removes only the bundle mapping and returns what was removed. Component packages, grants
    /// and every other table are untouched, so callers own package removal and retention choices.
    pub fn remove_bundle(&self, bundle_id: &str) -> Result<Option<BundleRecord>, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        let Some(bundle) = read(&tx, bundle_id)? else {
            return Ok(None);
        };
        // Delete the children explicitly: foreign-key enforcement is not enabled on every
        // connection, and the mapping must not gain orphans from a caller that skips the cascade.
        tx.execute(
            "DELETE FROM plugin_bundle_components WHERE bundle_id = ?1",
            [bundle_id],
        )
        .map_err(failed)?;
        tx.execute(
            "DELETE FROM plugin_bundles WHERE bundle_id = ?1",
            [bundle_id],
        )
        .map_err(failed)?;
        tx.commit().map_err(failed)?;
        Ok(Some(bundle))
    }

    /// Removes the mapping rows of the named components and, when no component row remains, the
    /// bundle row itself. One transaction, so a partially uninstalled bundle never leaves a row
    /// naming a package that is no longer installed. Returns the mapping that still exists.
    ///
    /// Component packages, grants and every other table are untouched.
    pub fn remove_bundle_components(
        &self,
        bundle_id: &str,
        plugin_ids: &[String],
    ) -> Result<Option<BundleRecord>, StorageError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        if read(&tx, bundle_id)?.is_none() {
            return Ok(None);
        }
        {
            let mut delete = tx
                .prepare(
                    "DELETE FROM plugin_bundle_components WHERE bundle_id = ?1 AND plugin_id = ?2",
                )
                .map_err(failed)?;
            for plugin_id in plugin_ids {
                delete
                    .execute(params![bundle_id, plugin_id])
                    .map_err(failed)?;
            }
        }
        let remaining: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM plugin_bundle_components WHERE bundle_id = ?1",
                [bundle_id],
                |row| row.get(0),
            )
            .map_err(failed)?;
        let remaining_bundle = if remaining == 0 {
            tx.execute(
                "DELETE FROM plugin_bundles WHERE bundle_id = ?1",
                [bundle_id],
            )
            .map_err(failed)?;
            None
        } else {
            read(&tx, bundle_id)?
        };
        tx.commit().map_err(failed)?;
        Ok(remaining_bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SqliteSettingsStore;
    use tempfile::TempDir;

    fn open(dir: &TempDir) -> SqliteSettingsStore {
        SqliteSettingsStore::open(&dir.path().join("test.sqlite3")).unwrap()
    }

    fn component(
        plugin_id: &str,
        kind: &str,
        digest: char,
        package_version: &str,
    ) -> BundleComponentRecord {
        BundleComponentRecord {
            plugin_id: plugin_id.to_owned(),
            kind: kind.to_owned(),
            component_sha256: digest.to_string().repeat(64),
            package_version: package_version.to_owned(),
        }
    }

    fn bundle(id: &str) -> BundleRecord {
        BundleRecord {
            bundle_id: id.to_owned(),
            name: "Dolphin GameCube and Wii".to_owned(),
            version: "0.1.0".to_owned(),
            publisher_fingerprint: "sha256:publisher".to_owned(),
            manifest_sha256: "a".repeat(64),
            installed_at: 500,
            components: vec![
                component("org.example.dolphin", "provider", 'a', "0.2.0"),
                component("org.example.dolphin.launch", "extension", 'b', "0.1.0"),
            ],
        }
    }

    #[test]
    fn a_bundle_and_its_components_round_trip() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir).bundle_store();
        let expected = bundle("org.example.dolphin");
        let mut recorded = expected.clone();
        // Components are read in plugin-id order, independent of how they were recorded.
        recorded.components.reverse();
        store.record_bundle(&recorded).unwrap();

        assert_eq!(
            store.bundle("org.example.dolphin").unwrap(),
            Some(expected.clone())
        );
        assert_eq!(store.bundle("org.example.missing").unwrap(), None);
        assert_eq!(store.bundles().unwrap(), vec![expected.clone()]);
        assert_eq!(expected.components[0].package_version, "0.2.0");
        assert_eq!(expected.components[1].kind, "extension");
    }

    #[test]
    fn recording_a_bundle_is_all_or_nothing() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir).bundle_store();
        let mut first = bundle("org.example.first");
        first.components = vec![component("org.example.shared", "provider", 'a', "1.0.0")];
        store.record_bundle(&first).unwrap();

        // The second component collides with the bundle recorded above after its sibling and the
        // bundle row were already inserted; the whole record must roll back.
        let mut second = bundle("org.example.second");
        second.components = vec![
            component("org.example.second", "provider", 'b', "1.0.0"),
            component("org.example.shared", "extension", 'c', "1.0.0"),
        ];
        assert!(store.record_bundle(&second).is_err());
        assert_eq!(store.bundle("org.example.second").unwrap(), None);
        assert_eq!(store.bundle_for_plugin("org.example.second").unwrap(), None);
        assert_eq!(
            store.bundle_for_plugin("org.example.shared").unwrap(),
            Some(first)
        );
    }

    #[test]
    fn a_plugin_resolves_to_its_bundle_or_none() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir).bundle_store();
        let recorded = bundle("org.example.dolphin");
        store.record_bundle(&recorded).unwrap();

        assert_eq!(
            store
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            Some(recorded)
        );
        assert_eq!(
            store.bundle_for_plugin("org.example.standalone").unwrap(),
            None
        );
    }

    #[test]
    fn an_update_archives_the_previous_mapping_and_rollback_restores_it() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir).bundle_store();
        let first = bundle("org.example.dolphin");
        store.record_bundle(&first).unwrap();

        let mut second = first.clone();
        second.version = "0.2.0".to_owned();
        second.manifest_sha256 = "c".repeat(64);
        second.installed_at = 900;
        second.components = vec![
            component("org.example.dolphin", "provider", 'd', "0.3.0"),
            component("org.example.dolphin.launch", "extension", 'e', "0.2.0"),
        ];
        assert_eq!(
            store.update_bundle(&second, 700).unwrap(),
            first,
            "the previous mapping is returned for rollback"
        );
        assert_eq!(
            store.bundle("org.example.dolphin").unwrap(),
            Some(second.clone())
        );
        assert_eq!(
            store.previous_versions("org.example.dolphin").unwrap(),
            vec!["0.1.0".to_owned()]
        );
        assert_eq!(
            store
                .previous_bundle("org.example.dolphin", "0.1.0")
                .unwrap(),
            Some(first.clone())
        );

        // An unchanged update changes nothing.
        assert!(store.update_bundle(&second, 800).is_err());

        let restored = store
            .restore_bundle_version("org.example.dolphin", "0.1.0", 1_000)
            .unwrap()
            .expect("the archived version is restored");
        assert_eq!(restored, first);
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), Some(first));
        assert_eq!(
            store.previous_versions("org.example.dolphin").unwrap(),
            vec!["0.2.0".to_owned()],
            "the replaced version becomes the rollback target"
        );
        // A version that is not archived restores nothing.
        assert_eq!(
            store
                .restore_bundle_version("org.example.dolphin", "9.9.9", 1_000)
                .unwrap(),
            None
        );
    }

    #[test]
    fn a_duplicate_bundle_id_does_not_replace_the_first_mapping() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir).bundle_store();
        let recorded = bundle("org.example.dolphin");
        store.record_bundle(&recorded).unwrap();

        let mut replacement = recorded.clone();
        replacement.version = "0.2.0".to_owned();
        assert!(store.record_bundle(&replacement).is_err());
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), Some(recorded));
    }

    #[test]
    fn invalid_bundle_records_are_rejected_before_writing() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir);
        let bundles = store.bundle_store();
        let cases = [
            {
                let mut value = bundle("org.example.invalid");
                value.bundle_id.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.name.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.version.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.publisher_fingerprint.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.manifest_sha256.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.installed_at = -1;
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.components.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.components[0].plugin_id.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.components[0].kind = "store".to_owned();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.components[0].component_sha256.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.components[0].package_version.clear();
                value
            },
            {
                let mut value = bundle("org.example.invalid");
                value.components[1].plugin_id = value.components[0].plugin_id.clone();
                value
            },
        ];
        for value in cases {
            assert!(bundles.record_bundle(&value).is_err(), "{value:?}");
        }
        let rows: i64 = store
            .conn()
            .query_row("SELECT COUNT(*) FROM plugin_bundles", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0, "a rejected record writes nothing");
    }

    #[test]
    fn removing_component_rows_keeps_a_partial_mapping_and_drops_an_empty_one() {
        let dir = TempDir::new().unwrap();
        let store = open(&dir).bundle_store();
        let recorded = bundle("org.example.dolphin");
        store.record_bundle(&recorded).unwrap();

        // One component removed: its row and only its row disappears; the bundle keeps the rest.
        let remaining = store
            .remove_bundle_components("org.example.dolphin", &["org.example.dolphin".to_owned()])
            .unwrap()
            .expect("the bundle still has a component");
        assert_eq!(
            remaining.components,
            vec![component(
                "org.example.dolphin.launch",
                "extension",
                'b',
                "0.1.0"
            )]
        );
        assert_eq!(
            store.bundle_for_plugin("org.example.dolphin").unwrap(),
            None
        );
        assert_eq!(
            store
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            Some(remaining.clone())
        );

        // Removing the last component deletes the bundle row as well.
        let remaining = store
            .remove_bundle_components(
                "org.example.dolphin",
                &["org.example.dolphin.launch".to_owned()],
            )
            .unwrap();
        assert_eq!(remaining, None);
        assert_eq!(store.bundle("org.example.dolphin").unwrap(), None);

        // An unknown bundle or an unknown component id writes nothing.
        assert_eq!(
            store
                .remove_bundle_components("org.example.missing", &["x".to_owned()])
                .unwrap(),
            None
        );
        store.record_bundle(&recorded).unwrap();
        let unchanged = store
            .remove_bundle_components("org.example.dolphin", &["org.example.other".to_owned()])
            .unwrap();
        assert_eq!(unchanged, Some(recorded));
    }

    #[test]
    fn removing_the_mapping_leaves_other_persistence_untouched() {
        let dir = TempDir::new().unwrap();
        let settings = open(&dir);
        settings
            .conn()
            .execute(
                "INSERT INTO plugin_runtime_resources
                    (owner_key, runtime_id, revision, runtime_json)
                 VALUES ('owner', 'runtime', 1, '{}')",
                [],
            )
            .unwrap();
        let bundles = settings.bundle_store();
        let recorded = bundle("org.example.dolphin");
        bundles.record_bundle(&recorded).unwrap();

        assert_eq!(
            bundles
                .remove_bundle("org.example.dolphin")
                .unwrap()
                .as_ref(),
            Some(&recorded)
        );
        assert_eq!(bundles.bundle("org.example.dolphin").unwrap(), None);
        assert_eq!(
            bundles
                .bundle_for_plugin("org.example.dolphin.launch")
                .unwrap(),
            None
        );
        assert_eq!(bundles.remove_bundle("org.example.missing").unwrap(), None);

        let sentinel: i64 = settings
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM plugin_runtime_resources WHERE runtime_id = 'runtime'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(sentinel, 1, "an unrelated row is preserved");

        // The mapping is fully gone, so the id can be recorded again.
        bundles.record_bundle(&recorded).unwrap();
        assert_eq!(
            bundles.bundle("org.example.dolphin").unwrap(),
            Some(recorded)
        );
    }
}
