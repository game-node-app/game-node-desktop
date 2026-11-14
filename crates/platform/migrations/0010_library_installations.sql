-- The opaque installation identity plugins see for a library entry. One row per entry key
-- (`<provider>:<store id>`); the id is random, assigned once and never changed, so it survives
-- rescans, renames and match changes and reveals neither the store id nor a path. Rows are never
-- deleted, like the entries themselves. Nothing here is a credential.
CREATE TABLE library_installations (
    entry_key       TEXT PRIMARY KEY NOT NULL,
    installation_id TEXT NOT NULL UNIQUE
) STRICT;
