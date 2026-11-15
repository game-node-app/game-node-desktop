# Local state and recovery

The platform `DataPaths` adapter resolves these locations for the current user:

| Platform | Application data | Settings database | Linux-only fallback credential |
| --- | --- | --- | --- |
| Windows | `%LOCALAPPDATA%\GameNode\Desktop` | `gamenode.sqlite3` below application data | None; Windows Credential Manager only |
| Linux | `$XDG_DATA_HOME/gamenode-desktop`, or `~/.local/share/gamenode-desktop` | `gamenode.sqlite3` below application data | `private/session-token`, `private/provider-<provider id>.secret` and the legacy `private/steam-token` alias below application data |

The Linux fallback directory has mode `0700` and the credential files `0600`.
Only an absent Secret Service permits fallback; locked or failing services
produce an error. Credentials are never settings. The keyring/fallback adapter
handles tokens separately from SQLite.

Credential slots are named by the host, never by a plugin:

- The GameNode account session uses the `session` keyring account and
  `private/session-token` on Linux.
- Each provider's plugin sees the logical key `session`, mapped to that provider's own physical
  slot: keyring account `provider.<provider id>` and Linux file
  `private/provider-<provider id>.secret`. A provider cannot address another provider's slot or the
  GameNode session.
- Trusted descriptor data may name an existing physical slot instead. Steam keeps its historical
  `steam` keyring account and `private/steam-token` file, so existing Steam credentials need no
  migration or new sign-in.

Isolated sign-in profiles live under `private/login/<provider>` and are removed when each attempt
ends. Startup also removes the legacy profile directories listed by trusted descriptors, such as
`private/steam-login`.


The database contains `schema_migrations` (version, name and applied time),
`settings` (key/value) and the library cache tables below. The only keys
`settings` may hold are `renderer`, `window.x`, `window.y`, `window.width`,
`window.height`, `window.maximized`, `library.layout`, `library.excluded`, `account.last_id` and
`account.last_name`, `sync_enabled`, `sync_explained`, `playtime_source` and
`discovery.lastCheckedAt` (the `SETTING_KEYS` list in `db.rs`). A fresh database
defaults to GPU rendering, the default library layout and no games. No session
token, callback URL, or pending sign-in secret belongs in this database.

## Library cache (migration `0002_library.sql`)

| Table | Holds |
| --- | --- |
| `library_entries` | Every game a provider has detected, keyed `<provider>:<store id>`, with its install folder, extra scan fields, `installed`, `owned` and `family_shared` flags and first/last seen times. Rows are never deleted: a game a completed scan no longer finds is marked `installed = 0`, so its matches and cover return with it. |
| `library_installations` | The opaque installation id plugins see for each library entry, keyed by entry key. The id is 128 random bits assigned the first time a plugin projection needs it and never changed, so it survives rescans, renames and match changes without revealing the store id or a path. Rows are never deleted. |
| `library_display_overrides` | The display name a provider's plugin set for one of its own entries, keyed by entry key, with a per-entry revision that starts at 1 and only grows. The detected name stays in `library_entries`; clearing an override sets the name to `NULL` and keeps the row so the revision keeps counting. |
| `library_artwork_overrides` | The cover a provider's plugin set for one of its own entries, keyed by entry key: `file` names an image in the covers folder (`plugin-<entry>-<content>.jpg` or `.png`, decoded, shrunk to fit 600 x 900 and re-encoded by the host) that only that entry uses, with a per-entry revision that starts at 1 and only grows. The matched game's own cover is never touched; clearing an override sets `file` to `NULL` and keeps the row so the revision keeps counting. |
| `provider_scans` | Per provider, the time of the last successful scan and the last error, plus the last owned (`last_catalog_at`, `catalog_error`) and family (`last_family_at`, `family_error`) catalog sync. A failed scan or catalog phase records the error and keeps the previous facts. |
| `store_accounts` | The last connected account of a store provider: `provider`, `account_id`, optional `account_name` and optional `last_verified_at`. It holds no cookie, token or password. |
| `auto_matches` | The GameNode game ids the server reported for a store id, per `(category, uid)` and PC. An empty list means the server knows no game. |
| `chosen_matches` | The explicit match per `(account_id, entry_key)`, made by the user or, through the plugin API, by the entry's own provider plugin (`source` is `user` or `plugin`; a plugin never replaces a `user` choice). A null `game_id` means "no GameNode game". Each account keeps its own choices. |
| `favorites` | Each account's favorite GameNode games per `(account_id, game_id)`, with `is_favorite`, `pending` (a local change GameNode has not confirmed) and `updated_at`. A false, confirmed row is deleted. Favorites survive sign-out and stay hidden from other accounts. |
| `games` | GameNode game name, release year, cover URL and cover state (`cover_file`, `cover_failed_at`) for display without the network. |

Saving a provider's scan is one transaction: it either fully applies or leaves
the previous entries as they were. Migration `0006_store_accounts_and_catalog.sql`
adds the independent `owned`/`family_shared` entry facts, the four catalog sync
columns and `store_accounts`. Existing entries stay installed and start neither
owned nor shared; an upgraded database keeps matches, covers, playtime and
settings. A successful owned or family sync replaces only its own fact set, an
owned game wins over a family-shared one, and clearing the catalog leaves entries,
matches and covers in place. An unreadable ownership flag is treated as false and
the installed fact still applies.

`library.layout` is a JSON object with the scope, filters, sort, grouping,
collapsed groups per grouping, view mode, sidebar and detail pane widths and
whether the detail pane and sidebar are open. Saved views live per account in
`saved_views`, not in this PC-wide layout. The search text is never saved. Older
layouts with a provider selection, a single match state, the old playtime or
release values, or the `installedOnly` toggle are normalized on load. A layout
that cannot be read falls back to the default.

`account.last_id` and `account.last_name` record the last signed-in account so
the cached library can open before the session is verified. They hold no
credential. Signing out deletes both keys; the library cache, chosen matches,
covers, layout, saved views, hidden entries and account library cache stay for a
later sign-in by the same account. The next start without a session shows sign-in
rather than a cached library.

## Covers

Cover images are files, not database rows: `covers/<game id>.jpg` below the
application data directory (`DataPaths::covers_dir`). The folder is created on
first use. Files are written atomically and are kept when a game is uninstalled
or the user signs out. Unreferenced files are not cleaned up yet.

## Third-party plugin storage

The private key/value data of third-party plugins is stored outside the application database, in
`plugin-data/<publisher fingerprint>/<plugin id>/<profile id>/storage.sqlite3` below the application
data directory (`DataPaths::plugin_data_dir`). There is one small SQLite file per plugin and account
profile, created the first time the plugin writes a `PERSISTENT` value; reading never creates one.
Directory names are the identity with unsafe characters written as `%XX` (or a digest when the name
would be too long), so no name can leave the folder. The `entries` table holds each key, value,
revision and optional expiry, and `meta` the highest revision ever used so revisions keep growing
across deletions and restarts. Session-only values are never written to disk.

The same file holds the folders the user approved for the plugin's file access, in a `file_roots`
table (store schema version 2): per installation and kind, the folder (a canonical path, kept only
by the host), the volume and node it had when approved, the operations allowed, the library entries
that shared it, and when it was approved or revoked. These rows are not charged to the plugin's
quota, and the plugin never sees them: it receives only opaque handles.

The location is for the user and the host only: the plugin API never accepts or returns it. Nothing
cleans these folders up yet (unused-data cleanup and the uninstall keep/delete choice are separate
tasks), so removing a plugin leaves its data in place. A store written by a newer version is refused
and left untouched, and a damaged file is reported as unavailable, never replaced.

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

## PC ports (migration `0008_ports.sql`)

Migration `0008_ports.sql` adds Windows-user-owned PC ports state. It is additive: every
existing table and value is kept, and the new tables start empty. An older build refuses the
upgraded schema without modifying it.

- `port_catalog_snapshot`: the single last complete upstream listing, with `payload` (JSON),
  `fetched_at` and `entry_count`. A refresh replaces the row in one transaction, so a failed or
  partial refresh keeps the previous snapshot.
- `port_overlay`: the single newest valid GameNode overlay, with `version` (the overlay schema
  version), `etag` and the TOML `payload`.
- `port_install_jobs`: each install job's `id`, `catalog_id`, `slug`, `port_name`,
  `release_version`, lifecycle `state`, detailed `phase` (JSON, including the supplied-file
  slot), `install_folder`, the `created_paths` JSON journal, the nullable `registration_id` it
  commits, the `result_seen` badge flag, failure reason and capped tool output, the resolved
  `plan_json`/`release_json`, and its timestamps.
- `port_installations`: port provenance per DRM-free registration id (`catalog_id`, `slug`,
  `port_name`, `installed_version`, `install_folder`, `environment_json` and `job_id`). It has no
  foreign key to `manual_registrations`, so removing a registration keeps the provenance row and
  the files it describes; `job_id` is unique so a re-added registration is relinked instead of
  duplicated.
- `port_preferences`: the single PC ports preference row: `view_mode`, `filters_json` (never the
  search text) and the last chosen `install_parent` folder.

Catalog and overlay data are not credentials and contain no GameNode account or device
identifier. The tables belong to the OS user and are shared across GameNode accounts.


## Achievements (migration `0009_achievements.sql`)

| Table | Holds |
| --- | --- |
| `achievement_games` | One row per `(provider, store_account_id, entry_id)` ever read: `state` (`ready`, `none` or `failed`), the `language` of the last successful read, `last_success_at`, `last_attempt_at` and the last failure category. A failed read keeps a previous success's state and data. |
| `achievements` | The complete definitions of one game, keyed by `(provider, store_account_id, entry_id, achievement_id)`: name, description, hidden flag, icon URLs, unlocked state, optional unlock time and the store's schema `position`. Each complete read replaces a game's rows in one transaction. |

The cache is keyed by the connected store account, independently of any GameNode
account, and contains no credential. Explicit store sign-out and replacement by a
different store account delete that account's rows; session expiry and an
unreachable store keep them.

## Library organization and account library (migration `0010_library_organization.sql`)

Migration `0010_library_organization.sql` is additive: every existing table and
value is kept and the new tables start empty. An older build refuses the upgraded
schema without modifying it. All five tables are keyed by the GameNode
`account_id`, so signing out keeps an account's rows for its next sign-in and
never exposes them to another account.

| Table | Holds |
| --- | --- |
| `saved_views` | One named sidebar view per account: `name`, the JSON `payload` (scope, filters, sort, grouping and view mode), its `position` and `created_at`. Names are unique per account. |
| `hidden_entries` | The provider entry keys the account hid from the library, with `hidden_at`. Hiding one provider entry never hides another entry of the same game. |
| `account_library_entries` | The account's cached GameNode library entry per `game_id`: `entry_id`, nullable `status` and the JSON array of `collection_ids`. A status this build does not know is read as unknown rather than dropping the game. |
| `account_collections` | The account's cached GameNode collections: `collection_id`, `name`, nullable `default_status` and their sidebar `position`. |
| `account_library_reads` | The completion marker of the last complete account library read: `read_at`. Entries and collections written before a complete read are not shown without this row. |

A complete read replaces entries and collections together and writes the
completion marker in one transaction, so a failed or partial read keeps the
previous cache. An individual entry upsert or removal never creates or clears the
marker.

`discovery.lastCheckedAt` records the last completed check of the watched DRM-free
folders in unix seconds, so the status bar can show it after a restart.
