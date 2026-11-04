-- Foundation settings: renderer preference and window geometry, as key/value pairs.
-- Credentials, tokens and authentication callback URLs never belong in this database.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) STRICT;

INSERT INTO settings (key, value) VALUES ('renderer', 'gpu');
