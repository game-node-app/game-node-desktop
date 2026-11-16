-- Origin and space for each listed save version, plus the configurable retention default.
CREATE TABLE backup_version_meta (
    installation TEXT NOT NULL,
    profile      TEXT NOT NULL,
    backup_id    TEXT NOT NULL,
    reason       TEXT NOT NULL CHECK (reason IN ('afterSession','manual','recovery')),
    play_seconds INTEGER,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (installation, profile, backup_id)
) STRICT;

-- Legacy settings default to ten copies; per-installation overrides live on save profiles.
UPDATE settings
SET value = json_set(value, '$.retention', 10)
WHERE key = 'backup_settings'
  AND json_valid(value)
  AND json_extract(value, '$.retention') IS NULL;
