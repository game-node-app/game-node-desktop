# Design

## Context

See `proposal.md` for the motivation. The visual target and every measurement are in `design-reference/`
(start with its `README.md`). The implementer does not need Figma.

Current state that shapes the approach:

- **Shells.** `FloatingModal` (draggable/resizable, layer stack, focus trap, Escape, focus restore,
  persisted geometry by `geometryKey`) and `AppModal` (Mantine `Modal.Root` wrapper with the same layer
  stack). Each dialog styles its own header, body and footer, so spacing differs between dialogs.
- **Primitives.** The library redesign added `Button`, `Pill`, `Checkbox`, `Switch`, `SegmentedControl`,
  `SectionLabel`, `IconButton`, `Banner` and tokens (`text-primary`, `text-muted`, `canvas`,
  `border-strong/overlay/floating`, radii `thumb/segment/control/popover/palette`, font sizes
  `badge…heading`). The dialogs do not use them yet.
- **Core state machines own dialog state.** `LibraryMachine` owns the match dialog
  (`match_dialog.rs`), the add dialog and its run (`add_dialog.rs`, `add_run.rs`), suggestions
  (`suggest.rs`) and the account library cache (`membership.rs`). The DRM-free, backups, tools and
  achievements state live in the app engine (`crates/app/src/engine/*`) and `crates/core/src/drm_free.rs`,
  `achievements.rs`, `tools/`. Auth is `crates/core/src/auth.rs`. The UI reads snapshots through
  react-query and sends inputs through `commands.*` (async Tauri commands).
- **Data gaps found while planning:**
  - `MatchOption` has no confidence, platforms or reasons, and search items drop `platformsAbbreviations`.
  - `CollectionOption` has no count. `GameApi` cannot create collections. `POST /v1/collections` returns
    **no body** (201) and can fail with `403 { code: "collection_limit_reached" }`.
  - `close_add` abandons a running batch.
  - `Achievement` (plugin protocol and core) has no rarity.
  - `BackupVersion` has only `label` and `bytes`. Retention is hard-coded
    (`crates/platform/src/backups.rs:485`, `--full-limit 10`).
  - `Registration` has no cover. `Candidate` has no suggestion and no non-game flag. Ignoring a candidate
    always sets `ignored = true`.
  - `auth.rs` cannot reopen the browser for a pending attempt.
  - `DiscardChangesDialog` does not know which fields are dirty.
  - `ClaimConflictModal` shows only titles.
- Unarchived `redesign-desktop-library` (72/73) and `add-achievement-tracking` (24/26) still hold deltas.
  This change only adds requirements next to theirs, or modifies requirements they do not touch.

## Goals / Non-Goals

**Goals:**

- One dialog system (two shells and shared parts) that every listed dialog uses, so spacing, type and
  colors come from tokens and never differ per dialog.
- Every new piece of information in the Figma frames is backed by real data from core/host, with the
  same behavior in the mock host.
- Keep every existing behavior the specs require (restore preview, overlapping-save warning, profile
  override, tool uninstall/update, auth error and restore states, sync tray note), restyled into the
  new layout.

**Non-Goals:**

- "Use without an account" (separate proposal), per-user achievement progress, a backup storage quota,
  and a developer reason in the match preview. No data source exists for the last three. Rows simply
  omit them.
- Restyling the Settings modal itself, the command palette or library surfaces. Only
  `DiscardChangesDialog` from Settings changes.
- A discard confirmation for `LocalGameEditor` or `SaveProtectionModal` (not in Figma, not in specs).
- Performance tests, benchmarks or budgets.

## Decisions

### D1 — Shared dialog system (`ui/src/components/dialog/`)

- `DialogShell` composes `FloatingModal.Root/Overlay/Content/Header/Body/Footer/ResizeHandle` with the
  classes in `components.md` §1. Its props are `title`, `subtitle`, `leading` (e.g. cover),
  `footer`, `footerNote`, `geometryKey`, `defaultSize`, `minSize` and `testId`. `FloatingModal` keeps
  its API. Only the default classes of `Content`/`Header`/`CloseButton` change (`rounded-dialog
  border-border-overlay shadow-dialog`, drag icon, 16px close icon), so other users of `FloatingModal`
  pick up the new look without code changes.
- `ConfirmDialog` composes `AppModal` (`components.md` §2): `tone`, `icon`, `title`, `description`,
  `children`, `actions`, `size`, `initialFocusRef`. Danger and warning confirmations focus the
  non-destructive action first.
- New primitives in `components/ui/`: `Field`, `TextInput`, `SelectField` (Mantine `Combobox` restyled
  with `menuClasses.ts`), `TextArea`, `OptionCard`, `Radio` and `RadioCard` (Mantine `Radio` restyled,
  so arrow keys work), `ProgressBar`, `Callout`, `Checklist`, `InsetCard`. Each one gets a Vitest file.
- Tokens: add only what `tokens.md` lists as new (`shadow-dialog`, `rounded-dialog`, `text-dialog-title`,
  `text-display`, `text-hero`, `leading-note/field/body-relaxed/hero`, `bg-signin-side/bottom`).
- *Alternative:* restyle each dialog in place. Rejected because it repeats the drift this change fixes.

### D2 — Match dialog: select, preview, confirm

- Core: `MatchDialog` gains `selected: Option<u64>`. New inputs `selectMatch { gameId }` and
  `confirmMatch { now }`. `pickMatch` stays as the confirm path (`confirmMatch` calls it with
  `selected`). On open, and when results arrive, the selection becomes the current match, or else the
  most confident option. A user's selection is never overwritten by a later result.
- Scoring: reuse `suggest.rs` (`title_score`, `confidence`, `tone`, `years_in`) for **every** option,
  without the 40% threshold. Candidates are ordered by confidence. Search results keep the server's
  order (the user asked for them).
- `MatchOption` adds `confidence: u8`, `tone: SuggestionTone`, `platforms: Vec<String>` and
  `reasons: Vec<MatchReason>`. `MatchReason` is one of `TitleSimilarity { percent }` (positive when
  ≥ 80, warning otherwise), `SameYear`, `DifferentYear { year }` (only when a year was detected),
  `StoreIdRecognized` (only for server candidates) and `ChosenByYou` (the current chosen match).
- Platforms: `GameSummary` adds `platforms: Vec<String>`, parsed from search
  `platformsAbbreviations` (a comma-separated string, split and trimmed). Lookup and stored games have
  none (an empty vector), so the meta line just omits them. Add `platforms` to the `GameRecord`
  persistence with a default, so a stored game keeps platforms it already received.
- Footer: Not on GameNode (ghost, `IconCircleX`), Use the automatic match (ghost, `IconSparkles`, only
  when `can_clear`), spacer, Cancel (closes), Match (primary, `IconLink`, disabled until a selection).
  Double-click or Enter on an option confirms. The options list is a `listbox` with arrow keys.
- *Alternative:* keep one-click pick. Rejected because the design needs the preview and an explicit
  confirmation, and a mis-click currently changes the match.

### D3 — Add dialog: status cards, counts, quick create, background batch

- Five status cards (`grid-cols-5`): Figma shows four, but `Ongoing` is a real GameNode status
  (`IconInfinity`). The default-status note uses the existing `applied_status` logic.
- Counts: `CollectionOption` adds `entryCount: Option<u32>`, computed in `add_dialog_view` from the
  account library cache (`membership.rs`: entries whose `collection_ids` contain the id). It is `None`
  when no complete read exists.
- Quick create: `GameApi::create_collection(name) -> Result<(), ApiError>` (`POST v1/collections` with
  `{ name }`, the server's defaults for visibility). Because the server returns no body, the core then
  re-lists collections and picks the id that is new **and** has that name. If none matches, it falls back
  to the newest same-name id and checks it. Core inputs: `startNewCollection`, `setNewCollectionName`,
  `createCollection`, `cancelNewCollection`. The view adds
  `newCollection: { name, creating, error: Option<CreateCollectionError> }`.
  `CreateCollectionError` is `LimitReached | Failed | Rejected | Offline`; `LimitReached` is parsed from
  the 403 `code`. Creating does not touch entries. Collection creation is an explicit user action, so
  sync-off does not block it (matches the "actions the user explicitly confirms" rule).
- Background batch: when confirmed with several targets, the run moves from `AddDialog.stage` to
  `LibraryMachine.add_runs` (one active run). The dialog shows the run's live items while it is open.
  A new `hideAdd` input closes the dialog **without** abandoning the run (`close_add` still abandons
  single-game and editing flows). Items get a `Waiting | Adding | Added | AlreadyPresent | Skipped |
  Failed` view state. When the run ends, the existing multi-add notification (with Undo / View on
  GameNode) is raised. If the dialog is open at that point, the summary is also shown in it.
  `SetAccount`/sign-out stops the run: no new create effect is issued, and the notification counts the
  rest as not added.
- *Alternative:* keep the run in the dialog and block closing. Rejected because the design says it
  continues in the background.

### D4 — Achievements: counts, rarity, sort, refresh time

- Protocol: `Achievement` adds `global_percent: Option<u16>` in **hundredths of a percent** (keeps
  `Eq`). It is optional in serde, so old plugins still parse.
- Steam: after the definitions and states succeed, the plugin requests
  `ISteamUserStats/GetGlobalAchievementPercentagesForApp/v2/?gameid=<appid>`. It is public, needs no
  key, and its `percent` may be a number or a string. Results are joined by API name. A failure
  leaves `global_percent = None`, and the refresh result stays `Ready`. Core storage (`achievements.rs`)
  persists the field (migration: nullable column). When a refresh returns no rarity, the core keeps the
  previously stored values for the same ids.
- Core list view: `AchievementListView` adds `counts { all, unlocked, locked }`, `refreshedAt`,
  `hasRarity`, `providerName` and `hasProviderPage`. The list query takes a `sort: MostRecent | Rarest | MostCommon |
  Name`. Sorting runs in core on the worker (not in React). Rows without rarity sort last. Hidden
  locked items sort by their generic label.
- Page URL: Steam `https://steamcommunity.com/stats/<appid>/achievements`. A new async command,
  `openAchievementsPage(entryKey)`, resolves the URL in the host and opens it, so the UI never passes a
  URL (the same pattern as `open_gamenode_page`). Other providers get `hasProviderPage = false`, which
  hides the button.
- UI: keep `@tanstack/react-virtual` with a fixed 65px row. Sort uses a Mantine `Menu`. The filter
  segments show counts. "Rare" is a client-side label for values below 1000 (10%).

### D5 — Save protection: status card, retention, version origin, space used

- Origin: new local table `backup_version_meta(installation, profile, backup_id, reason, play_seconds,
  created_at)`. It is written when a backup succeeds:
  - `after_session` with the session length, taken from the job's `play_period` session.
  - `manual`, from Backup now.
  - `recovery`, from the restore's recovery step.
  `BackupVersion` adds `createdAt: Option<i64>` (from Ludusavi's backup time, else meta),
  `reason: Option<BackupReason>` and `playSeconds: Option<u32>`. Versions without meta show time and
  size only.
- Retention: `BackupSettings` per installation/profile adds `retention: u8`, with allowed values
  `5 | 10 | 20 | 50` and a default of 10. `crates/platform/src/backups.rs` passes
  `--full-limit <retention>` instead of the literal `10`. Lowering it prunes at the next successful
  changed backup (Ludusavi's behavior). Recovery snapshots stay in their separate protected destination
  as today. New command `setLocalBackupRetention(entryKey, retention)`.
- Space used: `historyBytes` and `historyCount`, summed in the engine from the listed versions.
- Layout keeps the restore preview view, the overlapping-saves banner and the profile override. The
  resolved save files and registry keys move into a collapsed "Arquivos protegidos" disclosure under
  the save folder, which keeps the existing `desktop-library` requirement. The switch replaces the old
  auto-backup checkbox and calls the existing `setLocalProfileAutomatic`.

### D6 — Game tools

The data already exists in `ToolModalView`. Changes are layout only:

- Status pill from `GameToolView.state`. Channel uses `SegmentedControl`. Version and proxy DLL use
  `SelectField`. Target uses a read-only input with Browse… (existing `selectToolTargetExe` with picker).
- Candidates use a `Radio` list from `inspection.candidates` (launcher label from `TargetCandidate.label`).
- The reset-config checkbox, uninstall and update stay.
- The danger callout shows only when `inspection.anticheat` detects something. The Install button
  stays disabled until `anticheatConfirmed`.
- The footer note uses `ToolRelease.sizeBytes`, plus "verified with SHA-256" when `sha256` is present,
  or "already downloaded" when `cachedPath` is set.

### D7 — DRM-free game settings and custom cover

- Layout per `raw/07`. "Advanced · process detection" is collapsed by default and opens automatically
  when its textareas hold values that differ from the inferred defaults.
- Custom cover: `Registration` adds `custom_cover: Option<String>` (a cache file name). New async
  command `pickLocalCover(entryKey)` opens the native image picker (platform), then reads, decodes and
  validates the file (PNG/JPEG/WebP, max 20 MB) on a worker. It re-encodes the image to WebP at no more
  than 600×800 and writes it to the cover cache dir as `local-<registration-id>-<hash>.webp`. It returns
  the file name as a **pending** value in the editor state. `editLocalGame` commits it, or `None` for
  "Use the GameNode cover".
  - Cancel deletes the orphan file.
  - Cover resolution for local entries checks `custom_cover` before the GameNode cover, everywhere
    `cover_file` is computed (library rows, detail, dialogs), so it is served over
    `gamenode-asset://localhost/cover/<file>` with no protocol change.
- *Alternative:* store the original path. Rejected because the spec says deleting the original must not
  matter.

### D8 — Discovery review: suggestions, likely non-games, rediscovery

- `Candidate` adds `suggestion: Option<SuggestionView>` and `likelyNonGame: bool`.
- Suggestions: with a verified session, the engine searches GameNode once per new candidate title
  (bounded queue, one request at a time, cached by fingerprint) and keeps `rank_suggestions(...)[0]`.
  Adding a candidate with a suggestion records it as an **automatic** match for the new entry. It is
  not a user choice, so later lookups may still improve it.
- Non-game flag: true when the candidate has no positive evidence (`Evidence` empty or only weak) and
  its selected executable or folder name matches the maintenance list already used by discovery
  filtering (setup, unins*, *redist*, vc_redist, dxsetup, installer, updater, crashreport, launcher-only
  tools). This reuses the same list, so filtering and flagging cannot drift apart.
- Default selection: when candidates first appear, `review_selected` contains every candidate except
  likely non-games.
- Ignore: `reviewSelectedLocalCandidates({ add, allowRediscovery })`. Ignoring with
  `allowRediscovery = true` deletes the candidate record so a later scan recreates it. With `false` it
  sets `ignored = true`, as today. The footer checkbox defaults to checked, as in Figma, and it is only
  enabled while some row is selected.
- Table: semantic `<table>`, with a mixed header checkbox. The executable select uses `SelectField`
  compact with `candidate.targets`.

### D9 — Confirmations

- **Remove** (`RemoveLocalGameModal`): two `RadioCard`s. "Remove and don't import again"
  (`allowRediscovery=false`) is **preselected**, which keeps today's safer default and the existing spec
  scenario. Figma's frame shows the other card selected, but that is sample state. The card order follows
  Figma. Store entries (`removeEntry`) keep their own wording from `copy.md`. The open-folder row
  becomes a ghost button in the actions.
- **Discard** (`DiscardChangesDialog`): `SettingsModal` exposes `pendingChanges: string[]` (i18n field
  labels). Today that is only the backup destination. Any future text field adds its label there. Focus
  starts on "Keep editing".
- **Claim conflict**: the view adds `storeEntry { title, provider, coverFile, playtimeSeconds }` from the
  library row of `storeKey`, and `registration { title, root, coverFile }`. Actions: Cancel (ghost,
  `cancelLocalClaim`), Use existing item (`resolveLocalClaim(false)`), Move tracking
  (`resolveLocalClaim(true)`).
- **Sync explanation**: the checklist is static copy that mirrors `desktop-sync` (what sync sends,
  what stays). Figma's "Status e coleções que você escolher" row is replaced. Statuses and collections
  are sent only through explicit actions, whatever the sync setting, so listing them as sync would
  mislead. Turn sync off calls `setSyncEnabled(false)` and then `dismissSyncExplanation`, in that order,
  from the same click. Keep sync on dismisses. The existing tray note stays under the list when
  background is supported.

### D10 — Sign-in screen and pending attempt

- Core auth: the `Pending` state keeps `browser_url: String` (the address the `BeginSignIn` effect
  opened). New input `reopenBrowser` emits `Effect::OpenBrowser { url }` while it is pending and the URL
  is known. It does not change `op`, nonce or expiry. A failure is reported as a transient
  `reopenFailed` flag on the view. `AuthView.pending` adds `canReopen: bool`.
- Pending UI: a `ConfirmDialog` (accent tone, `IconExternalLink`) over the signed-out view, with
  `awaitingBrowser` driving "Waiting for confirmation…" vs "Confirming your account…". Escape or Cancel
  sign-in dispatches `cancelSignIn`.
- Cover mural: new async command `signInCoverWall() -> Vec<String>` returns up to 25 cover file names
  already in the cover cache (most recently used first), read on a worker. The user's cached covers always
  come first. Any unused tile is filled with bundled real IGDB cover art recorded in
  `ui/src/assets/signin-covers/manifest.json` (title and IGDB CDN image URL per file), so a first install
  shows real games instead of neutral or fake `Game01` tiles. The native cover-cache worker seeds the same
  bundled files when the cache directory exists, and the mock's `mock-N.svg` placeholders are resolved to
  the bundled art by the mural helper. No network request or credential is needed. The mural is
  `aria-hidden`, static, and is hidden below 860px window width.
- Error, restore-failed and verifying states keep their logic and render as a `Banner` in the sign-in
  column.

### D11 — Mock host parity (`VITE_UI_MOCK=1`)

The mock gains fixtures and handlers for every new field and input:

- match confidence, reasons and platforms; select and confirm
- collection counts, create (including a `limitReached` name trigger) and the background batch with timed
  item progress
- achievement counts, rarity and sort
- backup reasons, retention and space
- custom cover pick (returns a bundled sample)
- discovery suggestions and non-game rows
- claim conflict detail, sync turn-off, auth reopen and the cover wall

`docs/development.md` lists the new interactions.

### D12 — Testing

- Rust unit tests:
  - match: scoring, reasons and selection rules
  - add: count derivation, create re-list matching, hide-and-continue, account change stops the run
  - achievements: sort orders, rarity merge, percent parsing
  - backups: retention argument, meta join, bytes sum
  - DRM-free: non-game flag, default selection, ignore with and without rediscovery, custom cover
    precedence and validation
  - auth: reopen keeps op and expiry, and is ignored when not pending
- Vitest: each new primitive, and each rewritten dialog (render, keyboard, focus restore, disabled
  states, i18n en/pt-BR), using the existing `testUtils.tsx`.
- Manual check in the mock against `screens/*.png`, plus native Windows acceptance for the image picker
  and the browser reopen.

### Intentional deviations from the screens

The implementation keeps these differences from `design-reference/screens/`, because behavior the
specs require (or real data) needs them. They are not defects in the visual check.

- **D02 — five status cards.** `Ongoing` is a real GameNode status (`IconInfinity`), so the row shows
  five cards where Figma shows four (see D3). In pt-BR the `Em andamento` label wraps onto a second
  line, and the cards grow together to keep the row aligned.
- **D05 — status disclosures.** The protection card keeps the save-support `Pill` (`Supported`,
  `Needs selection`, …) and the history keeps the backup job-state `Pill` (`Backup completed`,
  running, failed), so a support problem or a failed run is visible inside the dialog.
- **D06 — tool metadata.** The tool card keeps the license label and the source-page button, the
  channel row keeps the refresh button, and the anti-cheat callout lists the detected evidence paths.
  These come from the existing tool views and stay reachable for support and verification.
- **D09 — remove default.** "Remove and don't import again" stays preselected (see D9); Figma's frame
  shows the other card selected as sample state.
- **D12 — sync checklist.** The statuses/collections row is replaced by "Games detected on this PC and
  their matches", because statuses and collections are only sent through explicit actions (see D9).
- **D01/D08 — no developer line.** GameNode search returns no developer, so the option meta and the
  preview omit it, and there is no developer-mismatch match reason (see Goals/Non-Goals and D2).

## Risks / Trade-offs

- [The server's `POST /v1/collections` returns no id] → Re-list and match by name and novelty. If two
  same-name collections appear at once, the newest is picked and the user sees it checked before
  confirming.
- [Global percentages endpoint changes or is rate-limited] → Optional field. Refresh never fails
  because of it, and the previous values are kept.
- [Match option scoring makes every search heavier] → Scoring is pure and runs over at most one result
  page in core. No extra requests.
- [Discovery suggestions add GameNode searches] → One per new candidate, queued, cached by fingerprint,
  only with a verified session. Nothing is sent when offline.
- [A hidden batch run outlives the dialog] → Exactly one run at a time. A new Add while one runs opens
  the progress view of the running batch instead of starting another.
- [Custom cover files accumulate] → Replacing or clearing a cover deletes the old file, and cancel
  deletes pending files.
- [Sync checklist copy can drift from behavior] → Copy keys are grouped under `sync.explanation.*`, and
  the design references `desktop-sync`. A spec change there must update them.
- [FloatingModal default-size changes] → Saved geometry wins, so users who resized keep their size.
  New defaults apply only to unsaved keys.

## Migration Plan

- Local state migrations (additive, nullable): `achievements.global_percent`, `backup_version_meta`
  table, `backup_settings.retention` (default 10), `local_registrations.custom_cover`,
  `game_records.platforms` (empty), and candidate `suggestion` / `likely_non_game` (recomputed on the
  next scan if absent).
- The plugin protocol field is optional, so older plugin builds keep working without rarity.
- Rollback: the new columns are ignored by older builds. Retention falls back to 10.

## Open Questions

- Should collections created from the desktop be private by default? The plan uses the server default
  (public), as the web does. Changing it later is a one-field request change with no spec impact.
