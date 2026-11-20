-- The custom cover chosen for a DRM-free registration, stored in its JSON payload as a file name
-- in the cover cache. Additive only: existing rows keep using the GameNode cover (null).
UPDATE manual_registrations
SET payload = json_set(payload, '$.customCover', NULL)
WHERE json_valid(payload)
  AND json_type(payload, '$.customCover') IS NULL;
