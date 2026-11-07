//! SQLite persistence for game tool installations, file manifests, INI patches, and release caches.

use std::str::FromStr;
use std::sync::{Arc, Mutex};

use gamenode_core::ToolId;
use gamenode_core::ports::StorageError;
use gamenode_core::tools::{
    ProxyDllName, ToolChannel, ToolError, ToolFileRecord, ToolIniPatchRecord, ToolInstallState,
    ToolInstallation, ToolRelease, ToolStore,
};
use rusqlite::{Connection, OptionalExtension, params};

const INI_PATCH_PREFIX: &str = "__ini_patch__:";

fn failed(error: impl std::fmt::Display) -> ToolError {
    ToolError::Storage(StorageError::Failed(error.to_string()))
}

fn state_to_str(state: ToolInstallState) -> &'static str {
    match state {
        ToolInstallState::Installing => "Installing",
        ToolInstallState::Installed => "Installed",
        ToolInstallState::Updating => "Updating",
        ToolInstallState::Uninstalling => "Uninstalling",
    }
}

fn state_from_str(s: &str) -> Result<ToolInstallState, ToolError> {
    match s.trim().to_lowercase().as_str() {
        "installing" => Ok(ToolInstallState::Installing),
        "installed" => Ok(ToolInstallState::Installed),
        "updating" => Ok(ToolInstallState::Updating),
        "uninstalling" => Ok(ToolInstallState::Uninstalling),
        other => Err(ToolError::Failed(format!(
            "unknown tool install state: {other}"
        ))),
    }
}

fn channel_from_str(s: &str) -> Result<ToolChannel, ToolError> {
    ToolChannel::from_str(s).map_err(ToolError::Failed)
}

fn map_installation_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<ToolInstallation, ToolError>> {
    let entry_key: String = row.get(0)?;
    let tool_id_str: String = row.get(1)?;
    let channel_str: String = row.get(2)?;
    let version: String = row.get(3)?;
    let target_exe: String = row.get(4)?;
    let proxy_dll_str: Option<String> = row.get(5)?;
    let state_str: String = row.get(6)?;
    let installed_at: i64 = row.get(7)?;
    let updated_at: i64 = row.get(8)?;

    let channel = match channel_from_str(&channel_str) {
        Ok(c) => c,
        Err(e) => return Ok(Err(e)),
    };
    let state = match state_from_str(&state_str) {
        Ok(s) => s,
        Err(e) => return Ok(Err(e)),
    };
    let proxy_dll = proxy_dll_str.and_then(|s| ProxyDllName::parse_flexible(&s));

    Ok(Ok(ToolInstallation {
        entry_key,
        tool_id: ToolId::new(tool_id_str),
        channel,
        version,
        target_exe,
        proxy_dll,
        state,
        installed_at,
        updated_at,
    }))
}

fn collect_installation_rows<I>(rows: I) -> Result<Vec<ToolInstallation>, ToolError>
where
    I: IntoIterator<Item = rusqlite::Result<Result<ToolInstallation, ToolError>>>,
{
    let mut result = Vec::new();
    for row in rows {
        let inst = row.map_err(failed)??;
        result.push(inst);
    }
    Ok(result)
}

fn ini_patch_synthetic_path(patch: &ToolIniPatchRecord) -> String {
    format!(
        "{}{}:{}:{}",
        INI_PATCH_PREFIX, patch.config_path, patch.section, patch.key
    )
}

pub struct SqliteToolStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteToolStore {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        crate::db::lock(&self.conn)
    }

    /// Returns the most recent unix timestamp (in seconds) when release metadata was fetched
    /// for a given tool and channel, or None if no cached releases exist.
    pub fn last_release_fetch_time(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
    ) -> Result<Option<i64>, ToolError> {
        let conn = self.conn();
        let time: Option<Option<i64>> = conn
            .query_row(
                "SELECT MAX(fetched_at) FROM tool_releases_cache WHERE tool_id = ?1 AND channel = ?2",
                params![tool_id.as_str(), channel.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(failed)?;
        Ok(time.flatten())
    }
}

impl ToolStore for SqliteToolStore {
    fn list_installations(&self) -> Result<Vec<ToolInstallation>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT entry_key, tool_id, channel, version, target_exe, proxy_dll, state, installed_at, updated_at
                 FROM tool_installations
                 ORDER BY entry_key ASC, tool_id ASC",
            )
            .map_err(failed)?;
        let rows = stmt.query_map([], map_installation_row).map_err(failed)?;
        collect_installation_rows(rows)
    }

    fn list_for_entry(&self, entry_key: &str) -> Result<Vec<ToolInstallation>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT entry_key, tool_id, channel, version, target_exe, proxy_dll, state, installed_at, updated_at
                 FROM tool_installations
                 WHERE entry_key = ?1
                 ORDER BY tool_id ASC",
            )
            .map_err(failed)?;
        let rows = stmt
            .query_map([entry_key], map_installation_row)
            .map_err(failed)?;
        collect_installation_rows(rows)
    }

    fn get_installation(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Option<ToolInstallation>, ToolError> {
        let conn = self.conn();
        let res = conn
            .query_row(
                "SELECT entry_key, tool_id, channel, version, target_exe, proxy_dll, state, installed_at, updated_at
                 FROM tool_installations
                 WHERE entry_key = ?1 AND tool_id = ?2",
                params![entry_key, tool_id.as_str()],
                map_installation_row,
            )
            .optional()
            .map_err(failed)?;
        match res {
            Some(Ok(inst)) => Ok(Some(inst)),
            Some(Err(err)) => Err(err),
            None => Ok(None),
        }
    }

    fn save_installation(&self, installation: &ToolInstallation) -> Result<(), ToolError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tool_installations (
                entry_key, tool_id, channel, version, target_exe, proxy_dll, state, installed_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(entry_key, tool_id) DO UPDATE SET
                channel = excluded.channel,
                version = excluded.version,
                target_exe = excluded.target_exe,
                proxy_dll = excluded.proxy_dll,
                state = excluded.state,
                installed_at = excluded.installed_at,
                updated_at = excluded.updated_at",
            params![
                installation.entry_key,
                installation.tool_id.as_str(),
                installation.channel.as_str(),
                installation.version,
                installation.target_exe,
                installation.proxy_dll.map(|p| p.as_str()),
                state_to_str(installation.state),
                installation.installed_at,
                installation.updated_at,
            ],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn delete_installation(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM tool_installations WHERE entry_key = ?1 AND tool_id = ?2",
            params![entry_key, tool_id.as_str()],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn incomplete_installations(&self) -> Result<Vec<ToolInstallation>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT entry_key, tool_id, channel, version, target_exe, proxy_dll, state, installed_at, updated_at
                 FROM tool_installations
                 WHERE LOWER(state) IN ('installing', 'updating', 'uninstalling')
                 ORDER BY installed_at ASC",
            )
            .map_err(failed)?;
        let rows = stmt.query_map([], map_installation_row).map_err(failed)?;
        collect_installation_rows(rows)
    }

    fn list_files(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Vec<ToolFileRecord>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT entry_key, tool_id, relative_path, installed_sha256, backup_path,
                        original_sha256, prior_owner_tool_id, created_dirs_json
                 FROM tool_files
                 WHERE entry_key = ?1 AND tool_id = ?2 AND relative_path NOT LIKE '__ini_patch__:%'
                 ORDER BY relative_path ASC",
            )
            .map_err(failed)?;
        let rows = stmt
            .query_map(params![entry_key, tool_id.as_str()], |row| {
                let entry_key: String = row.get(0)?;
                let tool_id_str: String = row.get(1)?;
                let relative_path: String = row.get(2)?;
                let installed_sha256: String = row.get(3)?;
                let backup_path: Option<String> = row.get(4)?;
                let original_sha256: Option<String> = row.get(5)?;
                let prior_owner_str: Option<String> = row.get(6)?;
                let created_dirs_json: String = row.get(7)?;

                let created_dirs: Vec<String> =
                    serde_json::from_str(&created_dirs_json).unwrap_or_default();

                Ok(ToolFileRecord {
                    entry_key,
                    tool_id: ToolId::new(tool_id_str),
                    relative_path,
                    installed_sha256,
                    backup_path,
                    original_sha256,
                    prior_owner_tool_id: prior_owner_str.map(ToolId::new),
                    created_dirs,
                })
            })
            .map_err(failed)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(failed)?);
        }
        Ok(result)
    }

    fn save_files(&self, files: &[ToolFileRecord]) -> Result<(), ToolError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO tool_files (
                        entry_key, tool_id, relative_path, installed_sha256, backup_path,
                        original_sha256, prior_owner_tool_id, created_dirs_json, ini_patches_json
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, '[]')
                     ON CONFLICT(entry_key, tool_id, relative_path) DO UPDATE SET
                        installed_sha256 = excluded.installed_sha256,
                        backup_path = excluded.backup_path,
                        original_sha256 = excluded.original_sha256,
                        prior_owner_tool_id = excluded.prior_owner_tool_id,
                        created_dirs_json = excluded.created_dirs_json",
                )
                .map_err(failed)?;

            for file in files {
                let dirs_json = serde_json::to_string(&file.created_dirs).map_err(failed)?;
                stmt.execute(params![
                    file.entry_key,
                    file.tool_id.as_str(),
                    file.relative_path,
                    file.installed_sha256,
                    file.backup_path,
                    file.original_sha256,
                    file.prior_owner_tool_id.as_ref().map(|id| id.as_str()),
                    dirs_json,
                ])
                .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    fn update_file_backup_owner(
        &self,
        entry_key: &str,
        relative_path: &str,
        new_owner: Option<&ToolId>,
        backup_path: Option<&str>,
        original_sha256: Option<&str>,
    ) -> Result<(), ToolError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE tool_files
             SET prior_owner_tool_id = ?3,
                 backup_path = CASE WHEN ?4 IS NOT NULL THEN ?4 ELSE backup_path END,
                 original_sha256 = CASE WHEN ?5 IS NOT NULL THEN ?5 ELSE original_sha256 END
             WHERE entry_key = ?1 AND relative_path = ?2 AND relative_path NOT LIKE '__ini_patch__:%'",
            params![
                entry_key,
                relative_path,
                new_owner.map(|id| id.as_str()),
                backup_path,
                original_sha256,
            ],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn delete_files(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM tool_files
             WHERE entry_key = ?1 AND tool_id = ?2 AND relative_path NOT LIKE '__ini_patch__:%'",
            params![entry_key, tool_id.as_str()],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn list_ini_patches(
        &self,
        entry_key: &str,
        tool_id: &ToolId,
    ) -> Result<Vec<ToolIniPatchRecord>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT entry_key, tool_id, backup_path, ini_section, ini_key, ini_value, ini_prior_value
                 FROM tool_files
                 WHERE entry_key = ?1 AND tool_id = ?2 AND relative_path LIKE '__ini_patch__:%'
                 ORDER BY relative_path ASC",
            )
            .map_err(failed)?;
        let rows = stmt
            .query_map(params![entry_key, tool_id.as_str()], |row| {
                Ok(ToolIniPatchRecord {
                    entry_key: row.get(0)?,
                    tool_id: ToolId::new(row.get::<_, String>(1)?),
                    config_path: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    section: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    key: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    applied_value: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    prior_value: row.get(6)?,
                })
            })
            .map_err(failed)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(failed)?);
        }
        Ok(result)
    }

    fn list_all_ini_patches_for_entry(
        &self,
        entry_key: &str,
    ) -> Result<Vec<ToolIniPatchRecord>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT entry_key, tool_id, backup_path, ini_section, ini_key, ini_value, ini_prior_value
                 FROM tool_files
                 WHERE entry_key = ?1 AND relative_path LIKE '__ini_patch__:%'
                 ORDER BY relative_path ASC",
            )
            .map_err(failed)?;
        let rows = stmt
            .query_map(params![entry_key], |row| {
                Ok(ToolIniPatchRecord {
                    entry_key: row.get(0)?,
                    tool_id: ToolId::new(row.get::<_, String>(1)?),
                    config_path: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    section: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    key: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    applied_value: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    prior_value: row.get(6)?,
                })
            })
            .map_err(failed)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(failed)?);
        }
        Ok(result)
    }

    fn save_ini_patches(&self, patches: &[ToolIniPatchRecord]) -> Result<(), ToolError> {
        let mut conn = self.conn();
        let tx = conn.transaction().map_err(failed)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO tool_files (
                        entry_key, tool_id, relative_path, installed_sha256, backup_path,
                        original_sha256, prior_owner_tool_id, created_dirs_json, ini_patches_json,
                        ini_section, ini_key, ini_value, ini_prior_value
                     ) VALUES (?1, ?2, ?3, '', ?4, NULL, NULL, '[]', '[]', ?5, ?6, ?7, ?8)
                     ON CONFLICT(entry_key, tool_id, relative_path) DO UPDATE SET
                        backup_path = excluded.backup_path,
                        ini_section = excluded.ini_section,
                        ini_key = excluded.ini_key,
                        ini_value = excluded.ini_value,
                        ini_prior_value = excluded.ini_prior_value",
                )
                .map_err(failed)?;

            for p in patches {
                let synth_path = ini_patch_synthetic_path(p);
                stmt.execute(params![
                    p.entry_key,
                    p.tool_id.as_str(),
                    synth_path,
                    p.config_path,
                    p.section,
                    p.key,
                    p.applied_value,
                    p.prior_value,
                ])
                .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    fn delete_ini_patches(&self, entry_key: &str, tool_id: &ToolId) -> Result<(), ToolError> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM tool_files
             WHERE entry_key = ?1 AND tool_id = ?2 AND relative_path LIKE '__ini_patch__:%'",
            params![entry_key, tool_id.as_str()],
        )
        .map_err(failed)?;
        Ok(())
    }

    fn load_releases(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
    ) -> Result<Vec<ToolRelease>, ToolError> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT tool_id, channel, version, tag_name, name, published_at,
                        download_url, asset_name, size_bytes, sha256, cached_path
                 FROM tool_releases_cache
                 WHERE tool_id = ?1 AND channel = ?2
                 ORDER BY rowid ASC",
            )
            .map_err(failed)?;
        let rows = stmt
            .query_map(params![tool_id.as_str(), channel.as_str()], |row| {
                let tool_id_str: String = row.get(0)?;
                let channel_str: String = row.get(1)?;
                let version: String = row.get(2)?;
                let tag_name: String = row.get(3)?;
                let name: String = row.get(4)?;
                let published_at: String = row.get(5)?;
                let download_url: String = row.get(6)?;
                let asset_name: String = row.get(7)?;
                let size_bytes_raw: i64 = row.get(8)?;
                let sha256: Option<String> = row.get(9)?;
                let cached_path: Option<String> = row.get(10)?;

                let channel = ToolChannel::from_str(&channel_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                    )
                })?;

                Ok(ToolRelease {
                    tool_id: ToolId::new(tool_id_str),
                    channel,
                    version,
                    tag_name,
                    name,
                    published_at,
                    download_url,
                    asset_name,
                    size_bytes: size_bytes_raw as u64,
                    sha256,
                    cached_path,
                })
            })
            .map_err(failed)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(failed)?);
        }
        Ok(result)
    }

    fn save_releases(
        &self,
        tool_id: &ToolId,
        channel: ToolChannel,
        releases: &[ToolRelease],
    ) -> Result<(), ToolError> {
        let mut conn = self.conn();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let tx = conn.transaction().map_err(failed)?;
        {
            let mut existing_cached = std::collections::HashMap::new();
            {
                let mut stmt = tx
                    .prepare(
                        "SELECT version, cached_path
                         FROM tool_releases_cache
                         WHERE tool_id = ?1 AND channel = ?2 AND cached_path IS NOT NULL",
                    )
                    .map_err(failed)?;
                let rows = stmt
                    .query_map(params![tool_id.as_str(), channel.as_str()], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                    })
                    .map_err(failed)?;
                for r in rows {
                    let (v, p) = r.map_err(failed)?;
                    existing_cached.insert(v, p);
                }
            }

            tx.execute(
                "DELETE FROM tool_releases_cache WHERE tool_id = ?1 AND channel = ?2",
                params![tool_id.as_str(), channel.as_str()],
            )
            .map_err(failed)?;

            let mut insert = tx
                .prepare(
                    "INSERT INTO tool_releases_cache (
                        tool_id, channel, version, tag_name, name, published_at,
                        download_url, asset_name, size_bytes, sha256, cached_path, fetched_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                )
                .map_err(failed)?;

            for r in releases {
                let cached_path = r
                    .cached_path
                    .as_deref()
                    .or_else(|| existing_cached.get(&r.version).map(|s| s.as_str()));
                let size_bytes = i64::try_from(r.size_bytes).unwrap_or(i64::MAX);

                insert
                    .execute(params![
                        r.tool_id.as_str(),
                        channel.as_str(),
                        r.version,
                        r.tag_name,
                        r.name,
                        r.published_at,
                        r.download_url,
                        r.asset_name,
                        size_bytes,
                        r.sha256,
                        cached_path,
                        now,
                    ])
                    .map_err(failed)?;
            }
        }
        tx.commit().map_err(failed)
    }

    fn clear_cached_archives_metadata(&self) -> Result<usize, ToolError> {
        let conn = self.conn();
        let cleared = conn
            .execute(
                "UPDATE tool_releases_cache SET cached_path = NULL WHERE cached_path IS NOT NULL",
                [],
            )
            .map_err(failed)?;
        Ok(cleared)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SqliteSettingsStore;
    use gamenode_core::library::{LastAccount, LibraryStore};
    use tempfile::TempDir;

    fn open_test_store(dir: &TempDir) -> (SqliteSettingsStore, SqliteToolStore) {
        let db_path = dir.path().join("tools_test.sqlite3");
        let settings = SqliteSettingsStore::open(&db_path).unwrap();
        let tools = settings.tool_store();
        (settings, tools)
    }

    #[test]
    fn test_installation_crud_across_multiple_games_and_tools() {
        let dir = TempDir::new().unwrap();
        let (_settings, store) = open_test_store(&dir);

        let opti_id = ToolId::new("optiscaler");
        let fsr_id = ToolId::new("fsr4-rdna2-lite");

        let inst1 = ToolInstallation {
            entry_key: "steam:1091500".to_string(),
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "bin/x64/Cyberpunk2077.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 1000,
            updated_at: 1000,
        };

        let inst2 = ToolInstallation {
            entry_key: "steam:1091500".to_string(),
            tool_id: fsr_id.clone(),
            channel: ToolChannel::Nightly,
            version: "1.0.0-beta".to_string(),
            target_exe: "bin/x64/Cyberpunk2077.exe".to_string(),
            proxy_dll: None,
            state: ToolInstallState::Installed,
            installed_at: 1010,
            updated_at: 1010,
        };

        let inst3 = ToolInstallation {
            entry_key: "manual:game123".to_string(),
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.8".to_string(),
            target_exe: "Game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Winmm),
            state: ToolInstallState::Installed,
            installed_at: 900,
            updated_at: 900,
        };

        store.save_installation(&inst1).unwrap();
        store.save_installation(&inst2).unwrap();
        store.save_installation(&inst3).unwrap();

        // Check get_installation
        let retrieved = store
            .get_installation("steam:1091500", &opti_id)
            .unwrap()
            .expect("inst1 should exist");
        assert_eq!(retrieved, inst1);

        let not_found = store.get_installation("manual:game123", &fsr_id).unwrap();
        assert!(not_found.is_none());

        // Check list_for_entry
        let cp_tools = store.list_for_entry("steam:1091500").unwrap();
        assert_eq!(cp_tools.len(), 2);
        assert_eq!(cp_tools[0].tool_id, fsr_id);
        assert_eq!(cp_tools[1].tool_id, opti_id);

        let manual_tools = store.list_for_entry("manual:game123").unwrap();
        assert_eq!(manual_tools.len(), 1);
        assert_eq!(manual_tools[0].version, "0.7.8");

        // Check list_installations
        let all = store.list_installations().unwrap();
        assert_eq!(all.len(), 3);

        // Update installation (version and proxy DLL switch)
        let mut updated_inst1 = inst1.clone();
        updated_inst1.version = "0.7.10".to_string();
        updated_inst1.proxy_dll = Some(ProxyDllName::Version);
        updated_inst1.updated_at = 2000;
        store.save_installation(&updated_inst1).unwrap();

        let retrieved_updated = store
            .get_installation("steam:1091500", &opti_id)
            .unwrap()
            .unwrap();
        assert_eq!(retrieved_updated.version, "0.7.10");
        assert_eq!(retrieved_updated.proxy_dll, Some(ProxyDllName::Version));
        assert_eq!(retrieved_updated.updated_at, 2000);

        // Delete installation
        store
            .delete_installation("steam:1091500", &opti_id)
            .unwrap();
        assert!(
            store
                .get_installation("steam:1091500", &opti_id)
                .unwrap()
                .is_none()
        );
        let cp_remaining = store.list_for_entry("steam:1091500").unwrap();
        assert_eq!(cp_remaining.len(), 1);
        assert_eq!(cp_remaining[0].tool_id, fsr_id);
    }

    #[test]
    fn test_startup_inquiry_of_incomplete_states_vs_installed() {
        let dir = TempDir::new().unwrap();
        let (_settings, store) = open_test_store(&dir);

        let opti_id = ToolId::new("optiscaler");
        let fsr_id = ToolId::new("fsr4-rdna2-lite");
        let mfg_id = ToolId::new("mfg-unlocker");
        let addon_id = ToolId::new("custom-addon");

        let inst_clean = ToolInstallation {
            entry_key: "steam:100".to_string(),
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            target_exe: "game.exe".to_string(),
            proxy_dll: Some(ProxyDllName::Dxgi),
            state: ToolInstallState::Installed,
            installed_at: 100,
            updated_at: 100,
        };

        let inst_installing = ToolInstallation {
            entry_key: "steam:200".to_string(),
            tool_id: fsr_id.clone(),
            channel: ToolChannel::Stable,
            version: "1.0.0".to_string(),
            target_exe: "game.exe".to_string(),
            proxy_dll: None,
            state: ToolInstallState::Installing,
            installed_at: 200,
            updated_at: 200,
        };

        let inst_updating = ToolInstallation {
            entry_key: "steam:300".to_string(),
            tool_id: mfg_id.clone(),
            channel: ToolChannel::Nightly,
            version: "2.0.0".to_string(),
            target_exe: "game.exe".to_string(),
            proxy_dll: None,
            state: ToolInstallState::Updating,
            installed_at: 300,
            updated_at: 300,
        };

        let inst_uninstalling = ToolInstallation {
            entry_key: "steam:400".to_string(),
            tool_id: addon_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.5.0".to_string(),
            target_exe: "game.exe".to_string(),
            proxy_dll: None,
            state: ToolInstallState::Uninstalling,
            installed_at: 400,
            updated_at: 400,
        };

        store.save_installation(&inst_clean).unwrap();
        store.save_installation(&inst_installing).unwrap();
        store.save_installation(&inst_updating).unwrap();
        store.save_installation(&inst_uninstalling).unwrap();

        let incomplete = store.incomplete_installations().unwrap();
        assert_eq!(incomplete.len(), 3);
        assert_eq!(incomplete[0].state, ToolInstallState::Installing);
        assert_eq!(incomplete[0].tool_id, fsr_id);
        assert_eq!(incomplete[1].state, ToolInstallState::Updating);
        assert_eq!(incomplete[1].tool_id, mfg_id);
        assert_eq!(incomplete[2].state, ToolInstallState::Uninstalling);
        assert_eq!(incomplete[2].tool_id, addon_id);

        // Transition one from Installing to Installed
        let mut completed = inst_installing;
        completed.state = ToolInstallState::Installed;
        store.save_installation(&completed).unwrap();

        let incomplete_after = store.incomplete_installations().unwrap();
        assert_eq!(incomplete_after.len(), 2);
        assert_eq!(incomplete_after[0].tool_id, mfg_id);
        assert_eq!(incomplete_after[1].tool_id, addon_id);
    }

    #[test]
    fn test_file_records_prior_owner_and_backup_owner_transfer() {
        let dir = TempDir::new().unwrap();
        let (_settings, store) = open_test_store(&dir);

        let opti_id = ToolId::new("optiscaler");
        let fsr_id = ToolId::new("fsr4-rdna2-lite");

        // OptiScaler deployed dxgi.dll and backed up game original amd_fidelityfx_dx12.dll
        let opti_files = vec![
            ToolFileRecord {
                entry_key: "steam:100".to_string(),
                tool_id: opti_id.clone(),
                relative_path: "dxgi.dll".to_string(),
                installed_sha256: "sha_dxgi".to_string(),
                backup_path: None,
                original_sha256: None,
                prior_owner_tool_id: None,
                created_dirs: vec!["plugins".to_string()],
            },
            ToolFileRecord {
                entry_key: "steam:100".to_string(),
                tool_id: opti_id.clone(),
                relative_path: "amd_fidelityfx_dx12.dll".to_string(),
                installed_sha256: "sha_opti_fid".to_string(),
                backup_path: Some("/backups/base_amd_fidelityfx_dx12.dll".to_string()),
                original_sha256: Some("sha_game_orig".to_string()),
                prior_owner_tool_id: None,
                created_dirs: vec![],
            },
        ];

        store.save_files(&opti_files).unwrap();

        let listed_opti = store.list_files("steam:100", &opti_id).unwrap();
        assert_eq!(listed_opti.len(), 2);
        assert_eq!(listed_opti[0].relative_path, "amd_fidelityfx_dx12.dll");
        assert_eq!(
            listed_opti[0].backup_path.as_deref(),
            Some("/backups/base_amd_fidelityfx_dx12.dll")
        );
        assert_eq!(listed_opti[1].relative_path, "dxgi.dll");
        assert_eq!(listed_opti[1].created_dirs, vec!["plugins"]);

        // Higher layer FSR 4 swapper installed over OptiScaler's amd_fidelityfx_dx12.dll
        let fsr_files = vec![ToolFileRecord {
            entry_key: "steam:100".to_string(),
            tool_id: fsr_id.clone(),
            relative_path: "amd_fidelityfx_dx12.dll".to_string(),
            installed_sha256: "sha_fsr4_fid".to_string(),
            backup_path: Some("/backups/opti_amd_fidelityfx_dx12.dll".to_string()),
            original_sha256: Some("sha_opti_fid".to_string()),
            prior_owner_tool_id: Some(opti_id.clone()),
            created_dirs: vec![],
        }];
        store.save_files(&fsr_files).unwrap();

        // When OptiScaler is uninstalled, transfer the base backup ownership to FSR 4
        store
            .update_file_backup_owner(
                "steam:100",
                "amd_fidelityfx_dx12.dll",
                None, // new owner is None (base game)
                Some("/backups/base_amd_fidelityfx_dx12.dll"),
                Some("sha_game_orig"),
            )
            .unwrap();

        // Delete OptiScaler file records
        store.delete_files("steam:100", &opti_id).unwrap();
        assert_eq!(store.list_files("steam:100", &opti_id).unwrap().len(), 0);

        // FSR 4 files should now have base backup ownership transferred
        let listed_fsr = store.list_files("steam:100", &fsr_id).unwrap();
        assert_eq!(listed_fsr.len(), 1);
        assert_eq!(listed_fsr[0].prior_owner_tool_id, None);
        assert_eq!(
            listed_fsr[0].backup_path.as_deref(),
            Some("/backups/base_amd_fidelityfx_dx12.dll")
        );
        assert_eq!(
            listed_fsr[0].original_sha256.as_deref(),
            Some("sha_game_orig")
        );
    }

    #[test]
    fn test_ini_patches_crud_and_isolation_from_file_records() {
        let dir = TempDir::new().unwrap();
        let (_settings, store) = open_test_store(&dir);

        let opti_id = ToolId::new("optiscaler");
        let mfg_id = ToolId::new("mfg-unlocker");

        // Save a regular file for OptiScaler
        let opti_files = vec![ToolFileRecord {
            entry_key: "steam:100".to_string(),
            tool_id: opti_id.clone(),
            relative_path: "OptiScaler.ini".to_string(),
            installed_sha256: "sha_ini".to_string(),
            backup_path: None,
            original_sha256: None,
            prior_owner_tool_id: None,
            created_dirs: vec![],
        }];
        store.save_files(&opti_files).unwrap();

        // Save INI patches for OptiScaler and MFG Unlocker
        let patches = vec![
            ToolIniPatchRecord {
                entry_key: "steam:100".to_string(),
                tool_id: opti_id.clone(),
                config_path: "OptiScaler.ini".to_string(),
                section: "General".to_string(),
                key: "LogLevel".to_string(),
                applied_value: "2".to_string(),
                prior_value: Some("0".to_string()),
            },
            ToolIniPatchRecord {
                entry_key: "steam:100".to_string(),
                tool_id: mfg_id.clone(),
                config_path: "OptiScaler.ini".to_string(),
                section: "Plugins".to_string(),
                key: "LoadAsiPlugins".to_string(),
                applied_value: "true".to_string(),
                prior_value: Some("false".to_string()),
            },
        ];
        store.save_ini_patches(&patches).unwrap();

        // Verify list_files only returns the file record, not the INI patches
        let files = store.list_files("steam:100", &opti_id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "OptiScaler.ini");

        // Verify list_ini_patches returns only the tool's INI patch
        let opti_patches = store.list_ini_patches("steam:100", &opti_id).unwrap();
        assert_eq!(opti_patches.len(), 1);
        assert_eq!(opti_patches[0].key, "LogLevel");
        assert_eq!(opti_patches[0].applied_value, "2");
        assert_eq!(opti_patches[0].prior_value.as_deref(), Some("0"));

        let mfg_patches = store.list_ini_patches("steam:100", &mfg_id).unwrap();
        assert_eq!(mfg_patches.len(), 1);
        assert_eq!(mfg_patches[0].key, "LoadAsiPlugins");

        // Verify list_all_ini_patches_for_entry returns both
        let all_entry_patches = store.list_all_ini_patches_for_entry("steam:100").unwrap();
        assert_eq!(all_entry_patches.len(), 2);

        // Delete files for OptiScaler - INI patches MUST remain intact!
        store.delete_files("steam:100", &opti_id).unwrap();
        assert_eq!(store.list_files("steam:100", &opti_id).unwrap().len(), 0);
        assert_eq!(
            store.list_ini_patches("steam:100", &opti_id).unwrap().len(),
            1
        );

        // Now delete INI patches for OptiScaler
        store.delete_ini_patches("steam:100", &opti_id).unwrap();
        assert_eq!(
            store.list_ini_patches("steam:100", &opti_id).unwrap().len(),
            0
        );
        // MFG Unlocker patch still exists
        assert_eq!(
            store.list_ini_patches("steam:100", &mfg_id).unwrap().len(),
            1
        );
    }

    #[test]
    fn test_release_catalog_caching_and_clearing() {
        let dir = TempDir::new().unwrap();
        let (_settings, store) = open_test_store(&dir);

        let opti_id = ToolId::new("optiscaler");

        // Initially no fetch time
        assert_eq!(
            store
                .last_release_fetch_time(&opti_id, ToolChannel::Stable)
                .unwrap(),
            None
        );

        let stable_releases = vec![
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                tag_name: "v0.7.9".to_string(),
                name: "OptiScaler v0.7.9".to_string(),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                download_url: "https://example.com/0.7.9.7z".to_string(),
                asset_name: "OptiScaler-v0.7.9.7z".to_string(),
                size_bytes: 5242880,
                sha256: Some("sha_stable_1".to_string()),
                cached_path: Some("/cache/OptiScaler-v0.7.9.7z".to_string()),
            },
            ToolRelease {
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.8".to_string(),
                tag_name: "v0.7.8".to_string(),
                name: "OptiScaler v0.7.8".to_string(),
                published_at: "2025-11-01T00:00:00Z".to_string(),
                download_url: "https://example.com/0.7.8.7z".to_string(),
                asset_name: "OptiScaler-v0.7.8.7z".to_string(),
                size_bytes: 4800000,
                sha256: None,
                cached_path: None,
            },
        ];

        store
            .save_releases(&opti_id, ToolChannel::Stable, &stable_releases)
            .unwrap();

        // Check fetch timestamp is now populated
        let fetch_time = store
            .last_release_fetch_time(&opti_id, ToolChannel::Stable)
            .unwrap();
        assert!(fetch_time.is_some());
        assert!(fetch_time.unwrap() > 0);

        // Check load_releases
        let loaded = store.load_releases(&opti_id, ToolChannel::Stable).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].version, "0.7.9");
        assert_eq!(
            loaded[0].cached_path.as_deref(),
            Some("/cache/OptiScaler-v0.7.9.7z")
        );
        assert_eq!(loaded[1].version, "0.7.8");
        assert!(loaded[1].cached_path.is_none());

        // Simulate re-fetching releases from GitHub API without local cached_path
        let refreshed_releases = vec![ToolRelease {
            tool_id: opti_id.clone(),
            channel: ToolChannel::Stable,
            version: "0.7.9".to_string(),
            tag_name: "v0.7.9".to_string(),
            name: "OptiScaler v0.7.9".to_string(),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            download_url: "https://example.com/0.7.9.7z".to_string(),
            asset_name: "OptiScaler-v0.7.9.7z".to_string(),
            size_bytes: 5242880,
            sha256: Some("sha_stable_1".to_string()),
            cached_path: None, // incoming API has no local cached_path
        }];
        store
            .save_releases(&opti_id, ToolChannel::Stable, &refreshed_releases)
            .unwrap();

        // Verify existing cached_path was preserved
        let reloaded = store.load_releases(&opti_id, ToolChannel::Stable).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert_eq!(
            reloaded[0].cached_path.as_deref(),
            Some("/cache/OptiScaler-v0.7.9.7z")
        );

        // Clear cached archives metadata
        let cleared = store.clear_cached_archives_metadata().unwrap();
        assert_eq!(cleared, 1);

        let after_clear = store.load_releases(&opti_id, ToolChannel::Stable).unwrap();
        assert_eq!(after_clear.len(), 1);
        assert!(after_clear[0].cached_path.is_none());
    }

    #[test]
    fn test_account_independent_persistence_across_signout() {
        let dir = TempDir::new().unwrap();
        let (settings, store) = open_test_store(&dir);
        let library_store = settings.library_store();

        // Sign in account 1
        library_store
            .save_last_account(&LastAccount {
                id: "account_abc".to_string(),
                name: "Player One".to_string(),
            })
            .unwrap();
        assert!(library_store.load_last_account().unwrap().is_some());

        let opti_id = ToolId::new("optiscaler");

        // Install tool, files, INI patch, releases
        store
            .save_installation(&ToolInstallation {
                entry_key: "steam:1091500".to_string(),
                tool_id: opti_id.clone(),
                channel: ToolChannel::Stable,
                version: "0.7.9".to_string(),
                target_exe: "bin/game.exe".to_string(),
                proxy_dll: Some(ProxyDllName::Dxgi),
                state: ToolInstallState::Installed,
                installed_at: 1000,
                updated_at: 1000,
            })
            .unwrap();

        store
            .save_files(&[ToolFileRecord {
                entry_key: "steam:1091500".to_string(),
                tool_id: opti_id.clone(),
                relative_path: "dxgi.dll".to_string(),
                installed_sha256: "sha_dxgi".to_string(),
                backup_path: None,
                original_sha256: None,
                prior_owner_tool_id: None,
                created_dirs: vec![],
            }])
            .unwrap();

        store
            .save_ini_patches(&[ToolIniPatchRecord {
                entry_key: "steam:1091500".to_string(),
                tool_id: opti_id.clone(),
                config_path: "OptiScaler.ini".to_string(),
                section: "General".to_string(),
                key: "LogLevel".to_string(),
                applied_value: "1".to_string(),
                prior_value: None,
            }])
            .unwrap();

        store
            .save_releases(
                &opti_id,
                ToolChannel::Stable,
                &[ToolRelease {
                    tool_id: opti_id.clone(),
                    channel: ToolChannel::Stable,
                    version: "0.7.9".to_string(),
                    tag_name: "v0.7.9".to_string(),
                    name: "OptiScaler v0.7.9".to_string(),
                    published_at: "2026-01-01T00:00:00Z".to_string(),
                    download_url: "https://example.com/0.7.9.7z".to_string(),
                    asset_name: "OptiScaler.7z".to_string(),
                    size_bytes: 1024,
                    sha256: None,
                    cached_path: Some("/cache/OptiScaler.7z".to_string()),
                }],
            )
            .unwrap();

        // Sign out account 1 (clear_last_account)
        library_store.clear_last_account().unwrap();
        assert_eq!(library_store.load_last_account().unwrap(), None);

        // Verify tools database tables are completely intact across signout
        let inst = store
            .get_installation("steam:1091500", &opti_id)
            .unwrap()
            .expect("installation remains intact across signout");
        assert_eq!(inst.version, "0.7.9");

        let files = store.list_files("steam:1091500", &opti_id).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "dxgi.dll");

        let patches = store.list_ini_patches("steam:1091500", &opti_id).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].key, "LogLevel");

        let releases = store.load_releases(&opti_id, ToolChannel::Stable).unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(
            releases[0].cached_path.as_deref(),
            Some("/cache/OptiScaler.7z")
        );
    }
}
