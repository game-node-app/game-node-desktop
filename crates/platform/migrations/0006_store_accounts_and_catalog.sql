-- Steam catalog facts, per-provider catalog sync state and non-secret store account identity.
-- Existing library entries keep their installed facts; owned and family-shared start false.

ALTER TABLE library_entries
    ADD COLUMN owned INTEGER NOT NULL DEFAULT 0 CHECK (owned IN (0, 1));
ALTER TABLE library_entries
    ADD COLUMN family_shared INTEGER NOT NULL DEFAULT 0 CHECK (family_shared IN (0, 1));

ALTER TABLE provider_scans ADD COLUMN last_catalog_at INTEGER;
ALTER TABLE provider_scans ADD COLUMN catalog_error TEXT;
ALTER TABLE provider_scans ADD COLUMN last_family_at INTEGER;
ALTER TABLE provider_scans ADD COLUMN family_error TEXT;

-- The last connected account of a store provider. It holds no cookie, token or password.
CREATE TABLE store_accounts (
    provider          TEXT PRIMARY KEY NOT NULL,
    account_id        TEXT NOT NULL,
    account_name      TEXT,
    last_verified_at  INTEGER
) STRICT;
