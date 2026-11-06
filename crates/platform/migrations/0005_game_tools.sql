CREATE TABLE tool_installations (
    entry_key TEXT NOT NULL,
    tool_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    version TEXT NOT NULL,
    target_exe TEXT NOT NULL,
    proxy_dll TEXT,
    state TEXT NOT NULL,
    installed_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (entry_key, tool_id)
) STRICT;

CREATE TABLE tool_files (
    entry_key TEXT NOT NULL,
    tool_id TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    installed_sha256 TEXT NOT NULL,
    backup_path TEXT,
    original_sha256 TEXT,
    prior_owner_tool_id TEXT,
    created_dirs_json TEXT NOT NULL DEFAULT '[]',
    ini_patches_json TEXT NOT NULL DEFAULT '[]',
    ini_section TEXT,
    ini_key TEXT,
    ini_value TEXT,
    ini_prior_value TEXT,
    PRIMARY KEY (entry_key, tool_id, relative_path)
) STRICT;

CREATE TABLE tool_releases_cache (
    tool_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    version TEXT NOT NULL,
    tag_name TEXT NOT NULL,
    name TEXT NOT NULL,
    published_at TEXT NOT NULL,
    download_url TEXT NOT NULL,
    asset_name TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    sha256 TEXT,
    cached_path TEXT,
    fetched_at INTEGER NOT NULL,
    PRIMARY KEY (tool_id, channel, version)
) STRICT;
