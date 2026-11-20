# Proposal

## Why

The library was rebuilt on the 2026 desktop design (`redesign-desktop-library`), but every dialog it opens,
and the sign-in screen, still use the old layout. They have mixed spacing, plain checkboxes where a choice
needs explanation, and they leave out what the user needs to decide: how confident a match is, how many
games a collection holds, why a backup exists and what it costs on disk, or what sync actually sends.
Figma section `5369:2801` "02 · Diálogos e modais" redesigns those 14 surfaces and adds the missing
information. This change implements it, including the core, host and server-client work the new information
needs, so the dialogs match the library they open from.

## What Changes

- **Shared dialog system.** Add a work-surface shell for `FloatingModal` (drag indicator, title + subtitle,
  surface footer) and a `ConfirmDialog` built on `AppModal` (tone circle, title, description, content slot,
  actions), plus shared fields, option and radio cards, progress bar, callout, checklist and a new dialog
  shadow, radius alias, three font sizes, four line heights and two sign-in gradients. Every dialog below uses them.
- **Match dialog.** Choosing becomes select-then-confirm. A preview pane shows the chosen game with
  "Why we suggest it" reasons. Each option shows a confidence pill, its year and its platforms. Footer:
  Not on GameNode, Use the automatic match, Cancel and Match.
- **Add to GameNode.** Statuses become cards. Collections show how many games each holds. The user can
  create a collection without leaving the dialog. Adding several games shows a per-game progress list
  that keeps running in the background when the user hides it.
- **Achievements.** The window shows an unlocked-progress summary, filter segments with counts, a sort
  (most recent, rarest, most common, name), the global unlock rarity for Steam, the last refresh time and
  Open in Steam.
- **Local save protection.** One status card with an on/off switch, save folder, retention, used space, and
  a history where each copy says why it was made (after a play session and its length, manual, recovery).
  **Retention becomes configurable** (it was hard-coded to ten).
- **Game tools.** Channel, version and proxy DLL side by side. A radio list of detected executables. A danger
  callout that keeps Install disabled until the anti-cheat risk is confirmed. A download-size and
  verification note.
- **DRM-free game settings.** Cover and identity at the top, paths grouped, process detection collapsed
  under Advanced. **New: change a DRM-free game's cover** to a local image.
- **Discovered games.** A table review with the executable chosen per row and a GameNode match suggestion
  per row. Likely non-games are unchecked by default. "Allow rediscovery" applies to Ignore selected.
- **Confirmations.** Remove game uses two explicit radio choices and one destructive action. Discard
  changes lists what will be lost. The tracking conflict shows both entries side by side. The sync
  explanation lists what leaves the PC and what stays, and offers Turn sync off directly.
- **Sign-in.** A cover mural made from cached covers, completed with bundled real IGDB cover art when
  the cache has fewer than 25 covers, and wide provider buttons. A pending state can **reopen the
  browser** for the same attempt. "Use without an account" from the Figma frame is **out of scope** and
  goes in a separate proposal.
- Out of scope: per-user achievement progress ("Progresso 32/50"; no provider exposes it), a backup storage
  quota ("84 MB de 1 GB"; there is no quota), and a developer-mismatch match reason (GameNode search
  returns no developer).

## Capabilities

### New Capabilities

<!-- None: every behavior belongs to an existing capability. -->

### Modified Capabilities

- `desktop-game-matching`: the match dialog previews a selected option with its confidence, platforms and
  reasons, and matches only on confirmation.
- `desktop-account-library`: collection sizes and quick collection creation in the add dialog; batch add
  shows per-game progress and continues in the background.
- `desktop-achievements` (introduced by the pending `add-achievement-tracking` change): global rarity,
  filter counts and sorting in the full list.
- `desktop-save-backups`: configurable retention, a reason and play length for each version, and the space
  the history uses.
- `desktop-drm-free-games`: custom cover for DRM-free games; discovery review with match suggestions,
  likely-non-game flags and an optional rediscovery choice; the tracking conflict shows both entries.
- `desktop-auth`: a pending sign-in can reopen the browser for the same attempt.
- `desktop-sync`: the first-use explanation lists what is sent and what stays local, and can turn sync off.
- `desktop-settings`: the discard confirmation lists the pending changes.

## Impact

- **UI** (`ui/src`): new shared dialog components; rewrites of `MatchDialog`, `AddDialog`,
  `AchievementListModal`, `SaveProtectionModal`, `GameToolsModal`, `LocalGameEditor`, `DiscoveryModal`,
  `RemoveLocalGameModal`, `DiscardChangesDialog`, `ClaimConflictModal`, `SyncExplanationModal` and
  `SignedOutView`; theme tokens; i18n (en, pt-BR); the mock host (`VITE_UI_MOCK=1`) and Vitest tests.
- **Core** (`crates/core`): match option scoring and reasons, add-dialog collection counts, collection
  creation and background batch runs, achievement rarity and sorting, backup version metadata and
  retention, discovery suggestions and non-game flags, custom covers, auth reopen input.
- **API client** (`crates/api`): `POST v1/collections`; platforms on search results.
- **Plugins** (`crates/plugin-protocol`, `crates/plugin-steam`): an optional global unlock percentage on
  achievements, read from Steam's public global-percentages endpoint.
- **Platform** (`crates/platform`): Ludusavi retention from settings; image picker for covers.
- **App host** (`crates/app`): new or extended async Tauri commands, local-state migrations (backup
  version metadata, retention setting, custom cover, discovery suggestions).
- **Dependency:** the `desktop-achievements` delta adds to a capability that the pending
  `add-achievement-tracking` change creates. Archive that change first.
