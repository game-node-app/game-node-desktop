# Steam playtime counter

The `steam` provider reads `userdata/<accountid>/config/localconfig.vdf` under
its canonical Steam roots through the same read-only host-file port as scans.
The account folder identifies a local Steam account; it is independent of the
signed-in GameNode account, which owns the imported totals in GameNode's database.

The KeyValues path is
`UserLocalConfigStore/Software/Valve/Steam/apps/<appid>` (case-insensitive keys).
`Playtime` is a nonnegative integer in minutes, converted to seconds with checked
multiplication by 60. `LastPlayed` is a nonnegative Unix timestamp in seconds.
Per app, select the account with the latest valid `LastPlayed`; never sum local
accounts. Account folders and roots are processed in sorted order so ties are stable.

A missing file, malformed account, absent app, absent required field, negative
value or overflow produces no new total for that app. Existing totals stay
unchanged. Other account files remain readable. Diagnostics contain no file
contents, credentials or command lines.

Steam writes can be delayed. Read at startup, after every scan, immediately on
exit of a game, and at +1 minute and +5 minutes after exit. A missing or unchanged
read does not reset a stored total; later scheduled reads can observe the update.

Synthetic fixtures verify 754 minutes → 45240 seconds, case-insensitive paths,
latest-account selection, isolated malformed accounts, missing entries, overflow
and a file updated between reads. They need no account login.

Store is the default global source. Internal records local measured sessions
instead. Games without a supported counter always use Internal. Changing source
keeps both histories separate; imported Steam totals are never added to sessions.
