-- The last completed playtime submission pass per account. Settings shows it after a restart,
-- before another pass runs; no credential or server response body is stored.
CREATE TABLE sync_summaries (
    account_id     TEXT PRIMARY KEY NOT NULL,
    completed_at   INTEGER NOT NULL,
    accepted_games INTEGER NOT NULL,
    stores         INTEGER NOT NULL,
    held           INTEGER NOT NULL
) STRICT;
