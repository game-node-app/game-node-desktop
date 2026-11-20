-- The user's selected recipe is separate from the plugin-owned recipe and stays scoped to the
-- library entry. A stale row is retained so deleting or revoking a recipe never silently falls
-- back to the detected launch target.
CREATE TABLE library_launch_recipe_selection (
    entry_key TEXT PRIMARY KEY NOT NULL REFERENCES library_entries(key) ON DELETE CASCADE,
    owner_key TEXT NOT NULL,
    plugin_id TEXT NOT NULL,
    publisher_fingerprint TEXT NOT NULL,
    recipe_id TEXT NOT NULL,
    recipe_revision INTEGER NOT NULL CHECK (recipe_revision >= 1),
    base_target_revision INTEGER NOT NULL CHECK (base_target_revision >= 0),
    approved_plan_fingerprint TEXT NOT NULL DEFAULT ''
) STRICT;
