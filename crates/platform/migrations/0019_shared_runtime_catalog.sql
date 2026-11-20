-- Host-owned identity index for common runtime projections. Identity values are supplied only
-- from the verified package/profile resolver at the resource boundary.
CREATE TABLE plugin_runtime_owners (
    owner_key TEXT PRIMARY KEY NOT NULL,
    plugin_id TEXT NOT NULL,
    publisher_fingerprint TEXT NOT NULL,
    profile_id TEXT NOT NULL
) STRICT;

CREATE INDEX plugin_runtime_owners_profile
    ON plugin_runtime_owners (profile_id, owner_key);

CREATE TABLE plugin_runtime_common_state (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    revision INTEGER NOT NULL CHECK (revision >= 0)
) STRICT;

INSERT INTO plugin_runtime_common_state (singleton, revision) VALUES (1, 0);
