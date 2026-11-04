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

The foundation database contains `schema_migrations` (version, name and applied
time) and `settings` (key/value). Its only supported settings are `renderer`,
`window.x`, `window.y`, `window.width`, `window.height` and `window.maximized`.
A fresh database defaults to GPU rendering and has no game records. No session
token, callback URL, or pending sign-in secret belongs in this database.

Opening a fresh database creates migration history and applies the initial
settings migration. Each pending migration and its history row commit in one
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

The current platform repository has temporary-database tests for fresh creation,
reopen, durable settings, ordered upgrade, rollback and unchanged newer-schema
files. Window/UI composition is tracked separately in the foundation change.
