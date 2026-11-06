# Spec Delta

## ADDED Requirements

### Requirement: Persist per-game tool installations, backups and release cache metadata
The local database SHALL store installed tool records (`tool_installations`), file/backup manifests (`tool_files`), and release cache metadata (`tool_releases_cache`) via migration `0005_game_tools.sql`. Tool installations SHALL belong to the Windows user (`(entry_key, tool_id)`) across GameNode accounts, with backups under `<data_dir>/private/tool-backups/` and archives under `<data_dir>/tools/cache/`.

#### Scenario: Restart with an installed game tool
- **WHEN** OptiScaler is installed on a game and the application restarts
- **THEN** the game's installed tool version, channel, target executable, proxy DLL filename, installed file digests, and backed-up original file paths are restored from SQLite before any network request completes

#### Scenario: Switch GameNode account with an installed tool
- **WHEN** one GameNode account signs out and another signs in on the same PC
- **THEN** the installed tool state and `Uninstall` capability remain intact for that game installation

#### Scenario: Reconcile interrupted tool operation on startup
- **WHEN** the application starts and finds a tool installation record left in a transient `Installing`, `Updating`, or `Uninstalling` state due to a prior crash
- **THEN** startup recovery rolls back the incomplete operation using the recorded backup manifest so the game folder returns to a clean, known state
