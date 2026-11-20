-- Global unlock percentage in hundredths of a percent (420 = 4.2%). Nullable: providers that do
-- not publish rarity, and refreshed rows whose percentages request failed, keep NULL.
ALTER TABLE achievements ADD COLUMN global_percent INTEGER
    CHECK (global_percent IS NULL OR global_percent BETWEEN 0 AND 65535);
