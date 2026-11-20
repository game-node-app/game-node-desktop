-- The launch target a provider detected for one of its own library entries (task 7.7). The
-- serialized value carries the target's revision, the user's approval and the file identity the
-- platform recorded at approval. A target the next scan no longer reports keeps its row with
-- `available` false, so the entry can offer repair instead of silently forgetting it. Storing a
-- target never starts a process.
ALTER TABLE library_entries ADD COLUMN target_json TEXT;
