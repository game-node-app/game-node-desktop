# Proposal

## Why

GameNode Desktop currently lists only the Steam games installed on this PC. Players who own a
large Steam library cannot browse it, filter it, or match it to GameNode until each game is
installed locally. Playnite solves this with a Steam web sign-in that imports the whole account
library. GameNode should offer the same capability, without an API key and without weakening the
existing rule that credentials never live in ordinary application data.

## What Changes

- Add a Steam web sign-in: an isolated in-app browser window shows Steam's own login page, the app
  reads the resulting Steam session cookie and stores it in a separate protected credential slot.
- Add automatic session refresh: the app obtains a short-lived Steam web API token from the
  authenticated store page, uses it to read the account library, and asks for a new sign-in only
  when Steam rejects the session.
- Import all Steam games the account owns, including uninstalled and unplayed games, plus games
  shared with the account through Steam Family.
- Change the library model from "installed only" to three independent facts per entry: installed
  on this PC, owned by the Steam account, and shared through Steam Family. The library shows the
  union; an **Installed only** filter narrows it without changing the store selection.
- Show owned-only and family-shared state in the grid, list and detail pane, and show why Play,
  Install and playtime behave as they do for those entries.
- Add a Steam account card to Settings > Stores with connection status, account identity, owned,
  family and installed counts, last check, secure-storage disclosure, sign-in, refresh, retry and
  sign-out actions. A warning indicator on the Stores topic and a library banner point to it when
  re-authentication is needed.
- Persist the new ownership facts and catalog sync state through a schema migration. Keep the
  credential out of SQLite: Windows Credential Manager, Linux Secret Service, or the existing
  user-only private file fallback on Linux with an explicit plain-text warning.
- Keep all GameNode library membership changes explicit. Importing a Steam library never adds
  games to the user's GameNode account library automatically.

**Non-goals:** Steam API-key authentication, multiple simultaneous Steam accounts, remote playtime
for owned games, Steam Family playtime, automatic Add to GameNode, and store logins other than
Steam.

## Capabilities

### New Capabilities

- `desktop-store-accounts`: Connect a store account through an isolated web sign-in, protect and
  refresh its credential, expose connection status, and disconnect without touching installed
  games.

### Modified Capabilities

- `desktop-providers`: The Steam provider gains a catalog capability. It fetches owned and
  family-shared games from Steam and combines them with locally installed games under one provider
  id.
- `desktop-library`: The library shows owned and family-shared games, not only installed ones; adds
  the Installed filter, filter-aware counts, ownership indicators, and account-attention banners.
- `desktop-settings`: The Stores panel gains the Steam account card, connection states, storage
  disclosure, account actions, attention indicator and toasts.
- `desktop-local-state`: Library entries gain ownership and family-sharing state; provider scan
  state gains catalog sync results; the Steam credential uses a separate slot and private-file
  path; a migration preserves existing data.

## Impact

- **Core:** provider manifest and registry, library machine and store types, query/filter, view
  models, a new store-account state machine, tests.
- **Platform:** named credential slots, Steam private-file fallback, SQLite migration, library
  persistence, tests.
- **API:** Steam store-page session extraction, `IPlayerService/GetOwnedGames`, Steam Family
  endpoints, retries and HTML/JSON fixtures, tests.
- **App:** engine orchestration, Steam login window and window lifecycle, Tauri commands, snapshot,
  background work and tests.
- **UI:** Stores panel, settings navigation indicator, library toolbar filter, sidebar counts,
  grid/list indicators, detail pane, empty states, banners, English and Brazilian Portuguese
  strings, component tests.
- **Data/config:** `providers/steam.toml`, schema migration `0006`, new credential file name under
  the private data directory.
- **External services:** Steam's store page and Web API. These endpoints are unofficial, can
  change, and are called only with the user's own session.
- **Docs:** provider documentation, a Steam web-session reference, and the roadmap entry that
  currently defers owned-but-not-installed games.
