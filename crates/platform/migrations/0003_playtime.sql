CREATE TABLE playtime_sessions (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL,
    entry_key TEXT NOT NULL,
    game_id INTEGER,
    source TEXT NOT NULL,
    platform_id INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    checkpoint_at INTEGER NOT NULL,
    ended_at INTEGER
) STRICT;
CREATE TABLE playtime_totals (
    account_id TEXT NOT NULL,
    entry_key TEXT NOT NULL,
    total_seconds INTEGER NOT NULL,
    last_played_at INTEGER,
    accepted_game_id INTEGER,
    accepted_seconds INTEGER,
    rejected_seconds INTEGER,
    PRIMARY KEY (account_id, entry_key)
) STRICT;
