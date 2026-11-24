-- Bundle grouping only: component packages, grants and activation stay in their existing stores.
-- Deleting this mapping never removes a package, and a standalone import leaves no row here. A
-- component plugin id belongs to at most one bundle at a time, so the inventory and uninstall can
-- resolve a plugin to exactly one bundle; a second bundle must adopt or report a conflict first.
CREATE TABLE plugin_bundles (
    bundle_id             TEXT PRIMARY KEY NOT NULL,
    name                  TEXT NOT NULL,
    version               TEXT NOT NULL,
    publisher_fingerprint TEXT NOT NULL,
    manifest_sha256       TEXT NOT NULL,
    installed_at          INTEGER NOT NULL CHECK (installed_at >= 0)
) STRICT;

CREATE TABLE plugin_bundle_components (
    bundle_id        TEXT NOT NULL REFERENCES plugin_bundles(bundle_id) ON DELETE CASCADE,
    plugin_id        TEXT NOT NULL UNIQUE,
    kind             TEXT NOT NULL CHECK (kind IN ('provider', 'extension')),
    component_sha256 TEXT NOT NULL,
    package_version  TEXT NOT NULL,
    PRIMARY KEY (bundle_id, plugin_id)
) STRICT;
