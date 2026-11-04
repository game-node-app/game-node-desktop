-- Library cache: what the providers detected on this PC and how it maps to GameNode games.
-- Existing `settings` rows are untouched. Nothing here is a credential.

-- One row per detected game, keyed `<provider>:<store id>`. Rows are never deleted: a game a
-- completed scan no longer finds is marked `installed = 0`, so its matches and cover survive.
CREATE TABLE library_entries (
    key         TEXT PRIMARY KEY NOT NULL,
    provider    TEXT NOT NULL,
    store_id    TEXT NOT NULL,
    name        TEXT,
    install_dir TEXT NOT NULL,
    fields_json TEXT NOT NULL DEFAULT '{}',
    installed   INTEGER NOT NULL DEFAULT 1 CHECK (installed IN (0, 1)),
    first_seen  INTEGER NOT NULL,
    last_seen   INTEGER NOT NULL
) STRICT;

CREATE INDEX library_entries_by_provider ON library_entries (provider);

-- The last scan outcome per provider. A failed scan records its error and keeps the entries.
CREATE TABLE provider_scans (
    provider        TEXT PRIMARY KEY NOT NULL,
    last_success_at INTEGER,
    last_error      TEXT
) STRICT;

-- Server facts, per PC: the GameNode game ids an external id maps to (a JSON array of ids).
CREATE TABLE auto_matches (
    category      INTEGER NOT NULL,
    uid           TEXT NOT NULL,
    game_ids_json TEXT NOT NULL DEFAULT '[]',
    checked_at    INTEGER NOT NULL,
    PRIMARY KEY (category, uid)
) STRICT;

-- The user's own choices, per account. A NULL game id means "no GameNode game".
CREATE TABLE chosen_matches (
    account_id TEXT NOT NULL,
    entry_key  TEXT NOT NULL,
    game_id    INTEGER,
    chosen_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, entry_key)
) STRICT;

-- GameNode game metadata shown in the library, so it works offline. `cover_file` is a file
-- name under the per-user `covers/` folder.
CREATE TABLE games (
    game_id         INTEGER PRIMARY KEY NOT NULL,
    name            TEXT NOT NULL,
    release_year    INTEGER,
    cover_url       TEXT,
    cover_file      TEXT,
    cover_failed_at INTEGER
) STRICT;
