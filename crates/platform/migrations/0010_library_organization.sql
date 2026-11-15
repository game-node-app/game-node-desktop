-- Per-account library organization and the GameNode account library cache.
--
-- Additive only: every table created here starts empty and existing tables are untouched.
-- An older build refuses the newer schema without modifying it.

CREATE TABLE saved_views (
  account_id TEXT NOT NULL, id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL, payload TEXT NOT NULL,    -- JSON {scope, filters, sort, group, view}
  position INTEGER NOT NULL, created_at INTEGER NOT NULL,
  UNIQUE (account_id, name)
) STRICT;

CREATE TABLE hidden_entries (
  account_id TEXT NOT NULL, entry_key TEXT NOT NULL, hidden_at INTEGER NOT NULL,
  PRIMARY KEY (account_id, entry_key)
) STRICT;

CREATE TABLE account_library_entries (
  account_id TEXT NOT NULL, game_id INTEGER NOT NULL, entry_id TEXT NOT NULL,
  status TEXT, collection_ids TEXT NOT NULL,    -- JSON array
  PRIMARY KEY (account_id, game_id)
) STRICT;

CREATE TABLE account_collections (
  account_id TEXT NOT NULL, collection_id TEXT NOT NULL, name TEXT NOT NULL,
  default_status TEXT, position INTEGER NOT NULL,
  PRIMARY KEY (account_id, collection_id)
) STRICT;

CREATE TABLE account_library_reads (
  account_id TEXT PRIMARY KEY, read_at INTEGER NOT NULL
) STRICT;
