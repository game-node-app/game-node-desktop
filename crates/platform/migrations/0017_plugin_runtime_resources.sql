-- Plugin-owned runtime/interpreter descriptors and content bindings. Runtime records describe
-- already approved installed software; they do not install files. Bindings intentionally have no
-- foreign key to runtime records or games so runtime removal cannot delete content, save/history,
-- or the binding needed to explain and migrate that content later.
CREATE TABLE plugin_runtime_resources (
    owner_key TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    runtime_json TEXT NOT NULL,
    PRIMARY KEY (owner_key, runtime_id)
) STRICT;

CREATE TABLE plugin_runtime_content_bindings (
    owner_key TEXT NOT NULL,
    game_id INTEGER NOT NULL CHECK (game_id > 0),
    installation_id TEXT NOT NULL,
    content_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0),
    binding_json TEXT NOT NULL,
    PRIMARY KEY (owner_key, game_id, installation_id, content_id, runtime_id)
) STRICT;

CREATE INDEX plugin_runtime_content_bindings_game
    ON plugin_runtime_content_bindings (owner_key, game_id, installation_id);

CREATE INDEX plugin_runtime_content_bindings_runtime
    ON plugin_runtime_content_bindings (owner_key, runtime_id);

CREATE TABLE plugin_runtime_catalog_state (
    owner_key TEXT PRIMARY KEY NOT NULL,
    catalog_revision INTEGER NOT NULL CHECK (catalog_revision >= 0)
) STRICT;
