-- Per-account favorite games. `pending` marks a local change GameNode has not confirmed yet;
-- a false, confirmed row is deleted, so absence means "not a favorite and nothing to send".
-- Favorites belong to the GameNode game, not to one provider entry. Nothing here is a credential.
CREATE TABLE favorites (
    account_id  TEXT NOT NULL,
    game_id     INTEGER NOT NULL,
    is_favorite INTEGER NOT NULL CHECK (is_favorite IN (0, 1)),
    pending     INTEGER NOT NULL CHECK (pending IN (0, 1)),
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (account_id, game_id)
) STRICT;
