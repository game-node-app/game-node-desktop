# Design

## Context

See proposal.md for motivation and specs/desktop-achievements/spec.md for required behavior.

- Steam runs as an external plugin. The host drives it through `ProviderOperation` commands and receives `OperationResult`; each operation is gated by a capability in `crates/plugin-protocol/src/capability.rs`. The protocol is at API 1.3, and plugins negotiate down to `MIN_API_MINOR`.
- The plugin already holds the session logic: it validates the store cookie and derives a short-lived `webapi_token`, which it never persists (`docs/steam-web-session.md`).
- `store_accounts` is keyed by provider and is PC-wide, independent of the GameNode account. `playtime_sessions`, by contrast, is scoped by `account_id`.
- Migrations are at `0008`. Manifests use `deny_unknown_fields`, so a new key needs parser support.
- The library is cache-first, and engine state reaches React through snapshots bridged with TanStack Query.

## Goals / Non-Goals

**Goals:**
- Local data that can later serve a no-account mode without being migrated.
- A provider contract that Epic or GOG can implement without host changes.
- Library-scale refresh that stays well-behaved towards Steam.

**Non-Goals:**
- Any write path to GameNode, including a reserved queue or outbox.
- Combining achievements with play sessions.
- A library-wide achievements screen, sorting or filtering the library by achievements, and unlock notifications.

## Decisions

### D1. A new operation, not an extension of catalog sync

Add `PROVIDER_OPERATION_READ_ACHIEVEMENTS`, capability `achievements.read`, and `ACHIEVEMENTS_API_MINOR = 4`. `ProviderCommand` gains an optional request (game ids, language); `OperationResult` gains one outcome per requested game: `ready` (definitions and unlocks), `none`, or a failure category.

Catalog sync is a single account-wide operation, while achievements are per-game, frequently re-requested and priority-driven. Returning one outcome per game in the result keeps each batch all-or-nothing per game and avoids a second streaming transfer. A batch is capped at 20 games, which fits well within the 8 MiB message limit even with large games; the host also enforces per-game achievement and string limits.

Plugins that negotiate an older minor are simply not granted the capability.

### D2. Steam endpoints

For each app, the plugin calls two endpoints with `access_token` plus an empty `key` parameter
(`access_token` alone is rejected by these endpoints):

- `ISteamUserStats/GetSchemaForGame/v2` with `l=<language>`, for definitions.
- `ISteamUserStats/GetPlayerAchievements/v1` with `steamid`, for `apiname`, `achieved` and `unlocktime`.

Unlock states are joined to definitions by API name. An `unlocktime` of 0 on an unlocked achievement becomes "no time". Existing retry limits and failure categories in `crates/plugin-steam/src/http.rs` are reused, extended with a per-game `none` result. Exact error shapes for apps without stats and private profiles are pinned from real responses in the first implementation task, then captured as fixtures.

Alternative considered: Steam's local stats files. Rejected because they cover only part of the account, and two sources would need merging.

### D3. Storage

Migration `0009` adds:

- `achievement_games`: `(provider, store_account_id, entry_id)` primary key, with `state`, `language`, `last_success_at`, `last_attempt_at`, `failure`.
- `achievements`: the same three columns plus `achievement_id`, with `name`, `description`, `hidden`, `icon_url`, `icon_gray_url`, `unlocked`, `unlocked_at`, `position`.

Each game is replaced in one transaction. There is no `account_id` and no game match column, so a future no-account mode needs no data migration. Queries use the currently connected store account from `store_accounts`.

Sign-out and account replacement delete by `(provider, store_account_id)` in the same flow that clears owned and family facts, which matches existing store-account behavior.

### D4. Scheduler in the engine

One worker per provider owns a priority queue, and a batch runs only when no other batch is running:

| Priority | Trigger | Staleness rule |
|---|---|---|
| 1 | Selected game | Older than 1 hour |
| 2 | Tracked game exited | Always, after a 30 second settling delay |
| 3 | Library import | Never checked |
| 4 | Reconciliation | Older than 7 days; `none` games after 30 days |

Background batches are paced so a Steam library is read at about one game per second. Progress derives from `last_success_at`, so restarts need no separate import state. Rate limiting pauses background priorities for an increasing interval. An expired session stops the worker until the account reconnects.

The worker uses the plugin runtime's existing async operation path, so no blocking happens on the UI thread. Results are written through the existing storage worker pattern.

Alternative considered: on-demand reads only. Rejected because offline use would then only cover games the user had already opened.

### D5. Language

The request carries the app locale, mapped by the plugin to Steam's `english` or `brazilian`. A stored language that differs from the current locale makes the game stale at priority 1 when it's selected; the rest update through reconciliation. Cached text stays visible meanwhile.

### D6. UI data flow

The detail snapshot carries only a compact summary (state, unlocked, total, latest unlock, last success). The full list comes from an async Tauri command through TanStack Query and is invalidated by a summary revision, so large lists don't travel in snapshots.

The list uses `FloatingModal`, `@tanstack/react-virtual` and Mantine `SegmentedControl`, and icons use `<img loading="lazy" decoding="async">`.

Hidden-achievement masking happens in the engine's list response, so spoilers don't reach the WebView.

### D7. Icons

The asset protocol gains `gamenode-asset://localhost/achievement/<hash>`, where the hash is derived from the validated URL. The host downloads an icon when the list requests it and rejects non-HTTPS URLs or hosts outside a trusted Steam image-host list stored in host metadata, never in plugin data. Icons are capped in size, stored under the per-user data directory, and included in cache usage and clearing.

## Risks / Trade-offs

- [Steam Web API token access or response shapes differ from expectations] → Task 1.1 confirms them against a real session before dependent work begins.
- [Rate limits on token-based calls are unknown] → Conservative pacing, backoff, and on-demand work taking priority over the import.
- [Cold import takes a long time for very large libraries] → Selected games jump the queue, and progress is resumable.
- [Deleting on sign-out loses offline data] → Consistent with owned facts, and a new sign-in re-imports them.
- [The icon cache grows] → Icons are fetched only for viewed games, and users can clear them.
- [Steam reports late unlocks] → Reconciliation corrects anything the post-exit read missed.

## Migration Plan

Migration `0009` only adds tables and needs no backfill. Rollback follows the existing rule that newer schemas are rejected by older builds, and removing the feature needs no change to other data.

## Open Questions

- What to report to GameNode, and when. This is a future change and doesn't affect this design.
