-- Achievement definitions and unlock state, keyed by provider entry and store account. The data
-- is PC-wide and independent of a GameNode account; sign-out and account replacement delete it.

-- One row per game ever read. `state` is `ready` (definitions and unlocks), `none` (a successful
-- read found no achievements) or `failed` (never successfully read). A failure after a success
-- updates `failure`/`last_attempt_at` but keeps the previous state and last success.
CREATE TABLE achievement_games (
    provider         TEXT NOT NULL,
    store_account_id TEXT NOT NULL,
    entry_id         TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('ready', 'none', 'failed')),
    language         TEXT NOT NULL,
    last_success_at  INTEGER,
    last_attempt_at  INTEGER NOT NULL,
    failure          TEXT,
    PRIMARY KEY (provider, store_account_id, entry_id)
) STRICT;

CREATE INDEX achievement_games_by_success
    ON achievement_games (provider, store_account_id, last_success_at);

-- The complete definitions of one game, replaced together in one transaction. `position` is the
-- store's schema order; unlocked rows keep Steam's reported time when it has one.
CREATE TABLE achievements (
    provider         TEXT NOT NULL,
    store_account_id TEXT NOT NULL,
    entry_id         TEXT NOT NULL,
    achievement_id   TEXT NOT NULL,
    name             TEXT NOT NULL,
    description      TEXT NOT NULL,
    hidden           INTEGER NOT NULL CHECK (hidden IN (0, 1)),
    icon_url         TEXT NOT NULL,
    icon_gray_url    TEXT NOT NULL,
    unlocked         INTEGER NOT NULL CHECK (unlocked IN (0, 1)),
    unlocked_at      INTEGER,
    position         INTEGER NOT NULL,
    PRIMARY KEY (provider, store_account_id, entry_id, achievement_id)
) STRICT;

CREATE INDEX achievements_by_game
    ON achievements (provider, store_account_id, entry_id, position);
