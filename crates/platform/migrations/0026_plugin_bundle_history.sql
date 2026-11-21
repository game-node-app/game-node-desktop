-- Previous bundle mappings, kept only so a bundle update can be rolled back to the component set
-- and package versions that were active before it. The current mapping stays in plugin_bundles;
-- history never owns packages, grants or activation, and no other table references it.
CREATE TABLE plugin_bundle_history (
    bundle_id             TEXT NOT NULL,
    version               TEXT NOT NULL,
    name                  TEXT NOT NULL,
    publisher_fingerprint TEXT NOT NULL,
    manifest_sha256       TEXT NOT NULL,
    installed_at          INTEGER NOT NULL CHECK (installed_at >= 0),
    archived_at           INTEGER NOT NULL CHECK (archived_at >= 0),
    PRIMARY KEY (bundle_id, version)
) STRICT;

CREATE TABLE plugin_bundle_history_components (
    bundle_id        TEXT NOT NULL,
    version          TEXT NOT NULL,
    plugin_id        TEXT NOT NULL,
    kind             TEXT NOT NULL CHECK (kind IN ('provider', 'extension')),
    component_sha256 TEXT NOT NULL,
    package_version  TEXT NOT NULL,
    PRIMARY KEY (bundle_id, version, plugin_id),
    FOREIGN KEY (bundle_id, version)
        REFERENCES plugin_bundle_history(bundle_id, version) ON DELETE CASCADE
) STRICT;
