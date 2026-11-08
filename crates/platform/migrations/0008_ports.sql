-- PC ports: the cached catalog, the GameNode overlay, install jobs, provenance and view
-- preferences. Everything here belongs to the Windows user and is shared across GameNode
-- accounts. Nothing here is a credential. Existing tables are untouched.

-- The last complete upstream listing. There is at most one row, replaced in a single transaction
-- once a refresh has completed and validated, so a failed refresh keeps the previous snapshot.
CREATE TABLE port_catalog_snapshot (
    id          INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    payload     TEXT NOT NULL,
    fetched_at  INTEGER NOT NULL,
    entry_count INTEGER NOT NULL CHECK (entry_count >= 0)
) STRICT;

-- The newest valid GameNode overlay. `version` is its `schema_version`; `etag` is the HTTP ETag of
-- the copy it came from (NULL for the bundled copy). `payload` is the overlay TOML text.
CREATE TABLE port_overlay (
    id      INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    version INTEGER NOT NULL,
    etag    TEXT,
    payload TEXT NOT NULL
) STRICT;

-- One row per install job, from queueing to its result. `state` is the coarse lifecycle state
-- (`queued`, `running`, `cancelling`, `completed`, `cancelled`, `failed`, `interrupted`) and
-- `phase` the detailed phase (including the supplied-file slot) as JSON. `port_name` and
-- `release_version` are kept on the row so history and failure toasts do not depend on the entry
-- still being in the catalog. `created_paths` is a JSON array of the absolute paths the job
-- created; each path is recorded there before it is written, so startup recovery can remove
-- partial work. `registration_id` is set in the same transaction that writes the DRM-free
-- registration and marks the job completed. `plan_json` and `release_json` keep the resolved plan
-- and release for retries. `failure_output` is the capped extractor output.
CREATE TABLE port_install_jobs (
    id              TEXT PRIMARY KEY NOT NULL,
    catalog_id      TEXT NOT NULL,
    slug            TEXT NOT NULL,
    port_name       TEXT NOT NULL DEFAULT '',
    release_version TEXT,
    state           TEXT NOT NULL,
    phase           TEXT NOT NULL,
    install_folder  TEXT,
    created_paths   TEXT NOT NULL DEFAULT '[]',
    registration_id TEXT,
    result_seen     INTEGER NOT NULL DEFAULT 0 CHECK (result_seen IN (0, 1)),
    failure_reason  TEXT,
    failure_output  TEXT,
    plan_json       TEXT,
    release_json    TEXT,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    finished_at     INTEGER
) STRICT;

CREATE INDEX port_install_jobs_by_created ON port_install_jobs (created_at);

-- Port provenance, one row per installed port. `registration_id` is the DRM-free
-- `Registration.id`, not the library key. There is deliberately no foreign key: removing a
-- registration keeps this row so PC ports can still show the install folder and files, and
-- re-adding the same executable reuses its id. `environment_json` is the environment map applied
-- when launching the entry.
CREATE TABLE port_installations (
    registration_id   TEXT PRIMARY KEY NOT NULL,
    catalog_id        TEXT NOT NULL,
    slug              TEXT NOT NULL,
    port_name         TEXT NOT NULL,
    installed_version TEXT NOT NULL,
    install_folder    TEXT NOT NULL,
    environment_json  TEXT NOT NULL DEFAULT '{}',
    job_id            TEXT NOT NULL
) STRICT;

CREATE INDEX port_installations_by_catalog ON port_installations (catalog_id);

-- One provenance row per install job, so relinking a re-added registration replaces the row
-- instead of accumulating one row per attempt.
CREATE UNIQUE INDEX port_installations_by_job ON port_installations (job_id);

-- The PC ports view mode, its filters (never the search text) and the last chosen install parent
-- folder, so the view reopens the way the user left it.
CREATE TABLE port_preferences (
    id             INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    view_mode      TEXT NOT NULL DEFAULT 'grid',
    filters_json   TEXT NOT NULL DEFAULT '{}',
    install_parent TEXT,
    updated_at     INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
