-- Who made a stored match choice: the user in the match dialog, or a plugin for one of its own
-- entries. A plugin may replace its own choices but never the user's. Choices stored before this
-- migration were made by the user, which is the default.
ALTER TABLE chosen_matches
    ADD COLUMN source TEXT NOT NULL DEFAULT 'user' CHECK (source IN ('user', 'plugin'));
