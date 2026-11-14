# Proposal

## Why

GameNode Desktop shows what a player owns and how long they played, but not what they achieved in those games. Achievements are a commonly requested launcher feature, and the desktop app is moving towards offline-first use, so they must come straight from each store and live on this PC instead of depending on GameNode's server-side import and web connections.

## What Changes

- Add a provider capability that lets a store plugin return a game's achievement definitions and the connected store account's unlock state. Steam is the first and only implementation.
- Read Steam achievements with the existing Steam web session and its derived short-lived token. Steam's local `appcache/stats` files are deliberately not read, because they do not cover the whole account.
- Store achievements in local SQLite, keyed by provider entry and store account. The data does not depend on a GameNode account, a GameNode match, or the server.
- Keep the cache fresh with bounded background work: on selecting a game, after a tracked game exits, a paced import across the Steam library, and periodic reconciliation.
- Show an achievement summary in the game detail pane and the full list in a floating window, both served from the local cache and usable offline. Icons are cached locally when a game's achievements are viewed.
- Send nothing about achievements to GameNode. What to report, and when, is still undecided.

Out of scope: reporting to GameNode, the `/v1/playtime/sessions` endpoint, a session timeline or journal, a no-account local profile, achievement notifications, global rarity, and stores other than Steam.

## Capabilities

### New Capabilities

- `desktop-achievements`: Reading achievements from store providers, caching them locally per store account, refreshing them without blocking the app, and presenting them offline.

### Modified Capabilities

None. Manifest support, the plugin service and the detail-pane section are all new behavior owned by `desktop-achievements`. The `desktop-plugins` and `desktop-plugin-services` capabilities are still unsynced in `integrate-steam-plugin-into-main`, so they cannot be targeted by a delta yet.

## Impact

- **Plugin API:** `crates/plugin-protocol` gains an achievements capability, operation and bounded DTOs at a new API minor version; `crates/plugin-sdk` and the host broker in `crates/app/src/plugin_host` follow.
- **Steam plugin:** `crates/plugin-steam` gains two Steam Web API reads and declares the capability; `providers/steam.toml` declares the backend.
- **Core and storage:** `crates/core` gains the achievement model and refresh rules; `crates/platform/migrations` gains a new migration; `crates/app/src/engine` gains the scheduler and commands.
- **UI:** `ui/src/features/library` gains the detail-pane summary and list window; both locale catalogs and the mock host are updated.
- **Assets:** the custom asset protocol gains an achievement icon route, with downloads limited to trusted Steam image hosts.
- **Network:** new requests go to Steam only. No GameNode API changes.
