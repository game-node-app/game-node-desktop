-- The cover a provider's plugin set for one of its own library entries. `file` names an image in
-- the covers folder that this entry alone owns; the game's own cover stays untouched. Clearing an
-- override sets file to NULL and keeps the row, so `revision` (writes per entry, starting at 1)
-- only ever grows. Nothing here is a credential.
CREATE TABLE library_artwork_overrides (
    entry_key TEXT PRIMARY KEY NOT NULL,
    file      TEXT,
    revision  INTEGER NOT NULL CHECK (revision >= 1)
) STRICT;
