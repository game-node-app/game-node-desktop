# Local state and recovery

The platform `DataPaths` adapter resolves these locations for the current user:

| Platform | Application data | Settings database | Linux-only fallback credential |
| --- | --- | --- | --- |
| Windows | `%LOCALAPPDATA%\GameNode\Desktop` | `gamenode.sqlite3` below application data | None; Windows Credential Manager only |
| Linux | `$XDG_DATA_HOME/gamenode-desktop`, or `~/.local/share/gamenode-desktop` | `gamenode.sqlite3` below application data | `private/session-token` below application data |

The Linux fallback directory has mode `0700` and the credential file `0600`.
Only an absent Secret Service permits fallback; locked or failing services
produce an error. Credentials are never settings. The keyring/fallback adapter
handles tokens separately from SQLite.

The database contains `schema_migrations` (version, name and applied time),
`settings` (key/value) and the library cache tables below. The only keys
`settings` may hold are `renderer`, `window.x`, `window.y`, `window.width`,
`window.height`, `window.maximized`, `library.layout`, `library.excluded`, `account.last_id` and
`account.last_name`, `sync_enabled`, `sync_explained` and `playtime_source`
(the `SETTING_KEYS` list in `db.rs`). A fresh database
uses the default library layout and starts with no games. The `renderer` key is
obsolete compatibility data and does not enable a current UI preference. No session
token, callback URL, or pending sign-in secret belongs in this database.

## Library cache (migration `0002_library.sql`)

| Table | Holds |
| --- | --- |
| `library_entries` | Every game a provider has detected, keyed `<provider>:<store id>`, with its install folder, extra scan fields, `installed` flag and first/last seen times. Rows are never deleted: a game a completed scan no longer finds is marked `installed = 0`, so its matches and cover return with it. |
| `provider_scans` | Per provider, the time of the last successful scan and the last error. A failed scan records the error and keeps the entries. |
| `auto_matches` | The GameNode game ids the server reported for a store id, per `(category, uid)` and PC. An empty list means the server knows no game. |
| `chosen_matches` | The user's own match per `(account_id, entry_key)`. A null `game_id` means "no GameNode game". Each account keeps its own choices. |
| `games` | GameNode game name, release year, cover URL and cover state (`cover_file`, `cover_failed_at`) for display without the network. |

Saving a provider's scan is one transaction: it either fully applies or leaves
the previous entries as they were.

`library.layout` is a JSON object with the view mode, sidebar selection, sort,
sidebar and detail pane widths and whether the detail pane is open. The search
text is never saved. A layout that cannot be read falls back to the default.

`account.last_id` and `account.last_name` record the last signed-in account so
the cached library can open before the session is verified. They hold no
credential. Signing out deletes both keys; the library cache, chosen matches,
covers and layout stay for a later sign-in by the same account. The next start
without a session shows sign-in rather than a cached library.

## Covers

Cover images are files, not database rows: `covers/<game id>.jpg` below the
application data directory (`DataPaths::covers_dir`). The folder is created on
first use. Files are written atomically and are kept when a game is uninstalled
or the user signs out. Unreferenced files are not cleaned up yet.

Opening a fresh database creates migration history and applies every
migration. A step 1 database gains the library tables and keeps its settings;
a step 1 build refuses the upgraded database under its newer-schema rule and
leaves it untouched. Each pending migration and its history row commit in one
transaction, in ascending order. Reopening a current database leaves its values
intact. A failed migration rolls back its changes; earlier completed migrations
remain committed. The adapter rejects a foreign database and checks for a newer
schema before writing. It never deletes, resets or downgrades a database to
recover from an error.

If startup reports a database problem:

1. Close every GameNode process before inspecting or copying local files.
2. Preserve a backup of the entire application-data directory, including any
   SQLite journal or sidecar files. Treat `private/` as secret and exclude it from
   issue reports and diagnostic uploads.
3. Check directory ownership, available space and filesystem access. Retry with
   the same application version after correcting the underlying problem.
4. For a newer schema, use the application version that created it or a newer
   compatible version. Do not edit migration history to force an older binary.
5. For corruption or migration failures, retain the original and investigate a
   copy. Do not remove the database as a normal recovery procedure.

The platform repositories have temporary-database tests for fresh creation,
reopen, durable settings, ordered upgrade (including from a step 1 database),
rollback and unchanged newer-schema files, and for rescans, uninstalls, failed
scans, per-account matches, the layout and sign-out.

## Playtime (migration `0003_playtime.sql`)

`playtime_sessions` holds `id`, `account_id`, `entry_key`, nullable `game_id`,
`source`, `platform_id`, `started_at`, `checkpoint_at` and nullable `ended_at`.
Times are Unix seconds UTC. A session is written when it starts and checkpointed
at least once per minute. At startup, before tracking begins, open rows from all
accounts are closed at `checkpoint_at`; time after that checkpoint is not counted.
Rows whose end is at or before their start are dropped and logged. Sessions stay
in the local queue; this version never submits or drains them. Signing out keeps
the account's queue for its next sign-in.

`playtime_totals` holds `account_id`, `entry_key`, `total_seconds`, nullable
`last_played_at`, `accepted_game_id`, `accepted_seconds` and `rejected_seconds`.
Its primary key is `(account_id, entry_key)`. Accounts never load or update each
other's totals or sessions. Neither table contains process lists, executable
paths or command lines. No session credential is stored here.

The additional settings keys `sync_enabled` and `sync_explained` hold `1` or `0`.
Missing values mean sync on and explanation not dismissed. An unreadable
`sync_enabled` means off and explanation due again, preserving opt-out safety.
They apply to this PC across accounts and survive sign-out.

Upgrading from step 2 adds both tables without changing settings, library entries
or matches. Step 2 builds refuse the newer schema and leave it untouched.

`playtime_source` is a PC-wide setting: `store` (default) or `internal`. Missing
or invalid values select `store`. Games without a supported store counter always
use Internal. Changing source preserves store totals and internal sessions
separately, without merging or deleting either history.

## DRM-free installations and local protection

Migration `0004_drm_free_backups.sql` adds Windows-user-owned tables:

- `manual_registrations`: stable `id`, canonical `file_identity`, `removed` tombstone
  and serialized launch/tracking `payload`. A unique partial index rejects duplicate
  active file identities. Missing executables remain unavailable registrations.
- `discovery_watches`: watch `id` and configuration `payload`.
- `discovery_candidates`: stable `fingerprint` and review/ignore `payload`.
- `local_inferences`: account-scoped `account_id`, `installation_id`, evidence and
  automatic catalog `payload`; chosen matches retain their existing account scope.
- `save_profiles`: `id`, `installation_id`, `revision`, frozen definition `payload`.
- `backup_jobs`: `id`, `installation_id`, `profile_id`, `revision`, `play_period`,
  `destination`, `state` and result `payload`. A play-period identity deduplicates
  automatic work. Profile changes cancel obsolete queued jobs transactionally.
- `backup_destinations`: previously used local destination `path` values.

The `backup_settings` settings key stores independent automatic-backup control
and destination. Save profiles and protection belong to the OS user and survive
GameNode account changes. They never inherit a catalog rematch. Backup bytes use
`backups/manual/<installation UUID>/<profile ID>/`, outside SQLite; removing a
registration preserves archives. Recovery snapshots are retained separately from
ordinary history. Interrupted jobs remain durable and require reconciliation
against actual archives before success or retry. Older binaries refuse this
schema; uninstall keeps user data and archives under the existing keep-data policy.

The private helper configuration lives under `private/backups/<installation>/<profile>`
below the same desktop data directory. It has empty roots and secondary-manifest
lists, and selects exactly one canonical profile title. The helper never uses a
standalone Ludusavi configuration or cloud synchronization. The manifest cache is
`manifest/manifest.yaml` with `metadata.json` for ETag/check time/hash; failed data
updates retain the validated baseline/cache. Jobs freeze their profile definition.

A profile retarget creates a new profile identity. Profiles load in insertion order,
so the most recently selected profile is active while older definitions remain for
history and restore. Old destinations remain in `backup_destinations` and job payloads.
Before enabling a destination, canonical ancestor checks reject overlap with saves,
registered installations or watched roots, including destinations not created yet.

Helper mutations use a copied `.staging` namespace, never hard-linked mutable
mapping files. Successful verified promotion writes a `.journal`, renames active to
`.previous`, then staging to active. Startup execution reconciles interrupted renames.
The active `.gamenode-job.json` marker binds a committed operation to its durable job
id; replay returns that result rather than creating another snapshot. Interrupted
Running jobs return to Queued and require a fresh stopped observation before execution.

Restore preserves current affected files and registry data in a separate profile
namespace suffixed `_recovery_<UUID>`. The original namespace records its path and
preview in `.gamenode-restore-recovery.json`. Ordinary ten-version retention never
prunes those recovery namespaces. Recovery failure aborts restore; interrupted or
partial restores retain that record for manual recovery. See [DRM-free games](drm-free-games.md)
for controls and the same-physical-drive limitation.

## Game tools (migration `0005_game_tools.sql`)

Migration `0005_game_tools.sql` adds Windows-user-owned game tool tables:

- `tool_installations`: installed tool state per `(entry_key, tool_id)`, storing
  `channel`, `version`, `target_exe`, `proxy_dll`, lifecycle `state`, and timestamps.
- `tool_files`: deployed file and INI patch ledger per `(entry_key, tool_id, relative_path)`,
  including `installed_sha256`, `backup_path`, `original_sha256`, `prior_owner_tool_id` for
  layered companion/swapper ownership, `created_dirs_json`, and INI patch rollback metadata.
- `tool_releases_cache`: cached release metadata per `(tool_id, channel, version)` fetched from
  declared repositories.

Downloaded archives are cached under `tools/cache/<tool_id>/<channel>/<version>/` (`DataPaths::tools_cache_dir`),
and pre-tool file backups live under `private/tool-backups/<entry_key>/<tool_id>/` (`DataPaths::tool_backups_dir`).
Tool installations belong to the OS user across GameNode account changes, and startup reconciles
any transient `Installing`, `Updating`, or `Uninstalling` records using the backup ledger.

