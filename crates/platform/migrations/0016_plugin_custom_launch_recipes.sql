-- Host-owned, plugin-scoped custom launch recipes. The JSON contains only typed fields and opaque
-- host handles; executable paths and secret values are resolved at launch time, never persisted.
CREATE TABLE plugin_custom_launch_recipes (
    owner_key TEXT NOT NULL,
    recipe_id TEXT NOT NULL,
    installation_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    recipe_json TEXT NOT NULL,
    PRIMARY KEY (owner_key, recipe_id)
) STRICT;

CREATE INDEX plugin_custom_launch_recipes_installation
    ON plugin_custom_launch_recipes (owner_key, installation_id);
