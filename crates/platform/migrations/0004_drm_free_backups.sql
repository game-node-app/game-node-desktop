-- Windows-user data; only inferred catalog matches belong to an account.
CREATE TABLE manual_registrations (
    id TEXT PRIMARY KEY NOT NULL,
    file_identity TEXT NOT NULL,
    removed INTEGER NOT NULL CHECK (removed IN (0,1)),
    payload TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX manual_active_file_identity ON manual_registrations(file_identity) WHERE removed = 0;
CREATE TABLE discovery_watches (id TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL) STRICT;
CREATE TABLE discovery_candidates (fingerprint TEXT PRIMARY KEY NOT NULL, payload TEXT NOT NULL) STRICT;
CREATE TABLE local_inferences (
    account_id TEXT NOT NULL, installation_id TEXT NOT NULL,
    payload TEXT NOT NULL, PRIMARY KEY(account_id, installation_id)
) STRICT;
CREATE TABLE save_profiles (
    id TEXT PRIMARY KEY NOT NULL, installation_id TEXT NOT NULL,
    revision INTEGER NOT NULL, payload TEXT NOT NULL
) STRICT;
CREATE TABLE backup_jobs (
    id TEXT PRIMARY KEY NOT NULL, installation_id TEXT NOT NULL,
    profile_id TEXT NOT NULL, revision INTEGER NOT NULL,
    play_period TEXT, destination TEXT NOT NULL, state TEXT NOT NULL,
    payload TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX backup_play_period ON backup_jobs(installation_id,profile_id,revision,play_period) WHERE play_period IS NOT NULL;
CREATE TABLE backup_destinations (path TEXT PRIMARY KEY NOT NULL) STRICT;
