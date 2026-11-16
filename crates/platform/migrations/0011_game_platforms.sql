-- Platform abbreviations that GameNode's search reported for a game. They are kept so a stored
-- game keeps the platforms it already received. Additive only: existing rows start with none.
ALTER TABLE games ADD COLUMN platforms TEXT NOT NULL DEFAULT '[]';
