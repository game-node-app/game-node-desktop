-- Presentation overrides a provider's plugin wrote for one of its own library entries. The
-- detected name stays in library_entries; clearing an override sets display_name to NULL and keeps
-- the row, so `revision` (writes per entry, starting at 1) only ever grows. Nothing here is a
-- credential.
CREATE TABLE library_display_overrides (
    entry_key    TEXT PRIMARY KEY NOT NULL,
    display_name TEXT,
    revision     INTEGER NOT NULL CHECK (revision >= 1)
) STRICT;
