# Tasks

Read before starting:

- `design.md` (decisions D1–D12) and the specs under `specs/`.
- Everything in `design-reference/`:
  - `README.md` (frame index and rules)
  - `screens/*.png` (visual targets)
  - `raw/*.txt` (exact sizes; start with `raw/00-shared-shell.txt`)
  - `tokens.md`, `components.md`, `icons.md`, `copy.md`
- You do not need Figma access.

Rules for every group:

- Follow AGENTS.md:
  - Never block the UI thread. Tauri commands are async; blocking I/O and image decoding run on workers.
  - Use Tailwind token classes. Inline `style` is only for runtime values.
  - Use Mantine v9 for complex controls and Tabler icons.
  - Use `FloatingModal` for work surfaces and `AppModal` for confirmations.
  - Use `gamenode-asset://` with `<img loading="lazy" decoding="async">`.
  - Keep `@tanstack/react-virtual` for long lists.
- Every user-facing string a task adds goes into both `ui/src/i18n/en.ts` and `ui/src/i18n/pt-BR.ts` in the
  same task, using `copy.md`. `i18n.test.ts` stays green. Never ship the word "NOVO".
- Every new input, command and view field is mirrored in `ui/src/mock/host.ts` (and its fixtures) in the
  same group that adds it to the host, so `VITE_UI_MOCK=1 pnpm --dir ui dev` keeps working at every step.
- Keep existing `data-testid`s where the element survives. Update tests in the same task that changes them.
- Verification commands:
  - `cargo test --locked -p gamenode-core -p gamenode-api -p gamenode-platform -p gamenode-app`
  - `cargo test --locked -p gamenode-plugin-protocol -p gamenode-plugin-steam` (group 5)
  - `pnpm --dir ui test --run`
  - `pnpm --dir ui exec tsc --noEmit`
- No performance tests or benchmarks.

## 1. Tokens and shared dialog system

- [x] 1.1 Add the new tokens from `design-reference/tokens.md` › New tokens to `ui/src/styles/theme.css` and
  `ui/tailwind.config.ts`:
  - `--shadow-dialog`, `--radius-dialog`
  - `--font-size-dialog-title/display/hero`
  - lineHeight `note/field/body-relaxed/hero`
  - `--gradient-signin-side/bottom` as backgroundImage keys

  Verify with a token test (extend the existing theme test, or add `ui/src/styles/theme.test.ts`) that
  asserts each class resolves.
- [x] 1.2 Restyle the `FloatingModal` defaults (`Content`, `Header`, `CloseButton`) and the `AppModal` content
  per `components.md` §1–2: `rounded-dialog`, `border-border-overlay`, `shadow-dialog`, drag icon, 16px
  close icon. Verify that the existing `FloatingModal`/`AppModal` tests still pass and that new assertions
  check the drag icon and the close button `aria-label`.
- [x] 1.3 Add `DialogShell` (`ui/src/components/dialog/DialogShell.tsx`) with title, subtitle, leading slot,
  body, footer, footer note, `geometryKey`, `defaultSize` and `minSize`. Verify with a Vitest test that
  checks rendering, Escape-to-close, focus restore to the trigger, and that the footer note and actions
  render.
- [x] 1.4 Add `ConfirmDialog` (`ui/src/components/dialog/ConfirmDialog.tsx`) with tone circle, title,
  description, content slot, actions and initial focus. Verify with a Vitest test for each tone class,
  initial focus on the given ref, and Escape.
- [x] 1.5 Add `Field`, `TextInput`, `SelectField` (Mantine Combobox with `menuClasses.ts`) and `TextArea` to
  `ui/src/components/ui/` per `components.md` §3, and export them from the barrel. Verify with Vitest tests
  for label binding, focus border class, read-only, select keyboard choice and placeholder.
- [x] 1.6 Add `OptionCard`, `Radio` and `RadioCard` (Mantine `Radio` restyled) per `components.md` §4. Verify
  with Vitest tests for selected and unselected classes, arrow-key movement in a group, and
  `aria-checked`/`aria-selected`.
- [x] 1.7 Add `ProgressBar`, `Callout`, `Checklist` and `InsetCard` per `components.md` §5–8. Verify with
  Vitest tests for progressbar ARIA values, the width style, tone classes and checklist icons per kind.

## 2. Match dialog (D2)

- [x] 2.1 `crates/api`: read `platformsAbbreviations` from search items into `GameSummary.platforms`. Add the
  field to `crates/core/src/game_api.rs` and to `GameRecord` persistence with an empty default (migration).
  Verify with API parsing tests for a string list, a missing field and an empty string, and with a core
  store round-trip test.
- [x] 2.2 Core `match_dialog.rs`: add `selected`, the `selectMatch`/`confirmMatch` inputs, the selection
  rules, confidence/tone/platforms/reasons on `MatchOption`, and candidates ordered by confidence. Reuse
  `suggest.rs`. Verify with unit tests in `match_dialog/tests.rs`:
  - initial selection is the current match, else the best option
  - a user selection survives new results
  - each reason appears only under its condition
  - confirm with no selection does nothing
  - cancel leaves the entry unchanged
- [x] 2.3 Expose the new inputs through `commands.libraryInput`, the TS types in `ui/src/api/types.ts`, and the
  mock host (fixtures with three options at 96/74/38%). Verify with `ui/src/mock/host.test.ts` cases for
  select and confirm.
- [x] 2.4 Rewrite `ui/src/features/dialogs/MatchDialog.tsx` on `DialogShell` (820×540) per `raw/01` and
  `screens/D01-match-dialog.png`:
  - search field
  - list label with count
  - option listbox with cover, meta and confidence `Pill`
  - preview pane with "Why we suggest it" `Checklist`
  - footer: Not on GameNode, Use the automatic match, Cancel, Match

  Keep `useBatchMatchDriver`. Verify with updated `MatchDialog` Vitest tests:
  - selecting does not match
  - Match confirms
  - Enter and double-click confirm
  - arrow keys move the selection
  - error, no-results and blocked notices
  - pt-BR strings

## 3. Add to GameNode (D3)

- [x] 3.1 Core: `CollectionOption.entryCount` from the account library cache (`None` without a complete read).
  Verify with unit tests in `add_dialog/tests.rs` for counts and for the no-read case.
- [x] 3.2 `crates/api` + `GameApi`: `create_collection(name)` (`POST v1/collections`, `{ name }`), mapping a 403
  `collection_limit_reached` to `CreateCollectionError::LimitReached`. Verify with API tests against the
  mock server for 201 with an empty body, the 403 limit, a 401 rejection and a network failure.
- [x] 3.3 Core: new-collection inputs (`startNewCollection`, `setNewCollectionName`, `createCollection`,
  `cancelNewCollection`), the re-list and pick-the-new-id flow, auto-check of the new collection, a disabled
  blank name, and no entry mutation. Verify with unit tests:
  - success selects the new id
  - two same-name collections pick the new one
  - failure keeps the name and choices
  - limit error
  - offline
- [x] 3.4 Core: move multi-target runs to `LibraryMachine.add_runs`. Add the `hideAdd` input, per-item view
  states (waiting/adding/added/already/skipped/failed), the end-of-run notification while hidden, one run at
  a time (a new Add opens the running progress), and account change stopping the run. Verify with unit tests
  in `add_run/tests.rs`:
  - hide keeps issuing create effects
  - the notification is raised once
  - sign-out stops new effects and counts the rest
  - the single-game close still abandons
- [x] 3.5 Commands, TS types and mock host for 3.1–3.4. The mock includes a name that triggers `limitReached`
  and a timed three-item batch with one unmatched game. Verify with `host.test.ts` cases for create and for
  hide-and-continue.
- [x] 3.6 Rewrite `AddDialog.tsx` on `DialogShell` (520×560) per `raw/02`, `raw/03` and screens D02/D03:
  - header cover
  - five status `OptionCard`s (radiogroup)
  - default-status note
  - collections list with counts and an inline "New collection" field
  - progress mode with `ProgressBar`, the item list, the background note and Hide
  - edit mode title and Save

  Verify with Vitest tests:
  - status keyboard selection
  - count display and its hidden state
  - create flow: success, error and limit messages
  - progress rendering of each item state
  - Hide calls `hideAdd`
  - pt-BR strings

## 4. Achievements (D4)

- [x] 4.1 `crates/plugin-protocol`: optional `global_percent: Option<u16>` (hundredths) on `Achievement`, with
  serde default. Verify with protocol tests: old JSON without the field parses, and a round-trip keeps the
  value.
- [x] 4.2 `crates/plugin-steam`: fetch `GetGlobalAchievementPercentagesForApp/v2`, parse number-or-string
  percents, join by API name, and treat failure as `None` without failing the outcome. Verify with tests in
  `account.rs`: join, string percent, request failure keeps `Ready`, unknown names are ignored.
- [x] 4.3 Core `achievements.rs` and storage: persist rarity (nullable column migration), keep previous values
  when a refresh lacks rarity, and add counts, `refreshedAt`, `hasRarity`, `providerName`, `hasProviderPage`
  and the `sort` parameter (MostRecent, Rarest, MostCommon, Name; no-rarity rows last; hidden rows by their
  generic label). Verify with unit tests for each sort, the counts, the rarity merge and the migration.
- [x] 4.4 Host: `achievementList(entryKey, filter, sort)` and the new async `openAchievementsPage(entryKey)`,
  which resolves the Steam stats URL in the host. Add the commands, TS types and mock fixtures (58
  achievements with rarity, one hidden). Verify with an app command test that the URL resolves for Steam and
  is refused for other providers, and with mock host tests.
- [x] 4.5 Rewrite `AchievementListModal.tsx` on `DialogShell` (640×600) per `raw/04` and screen D04:
  - summary `InsetCard` with `ProgressBar`
  - counted segments
  - sort `Menu` (rarity options hidden without rarity)
  - virtualized 65px rows with icon, lock fallback, date or "Locked", rarity line ("Rare" below 10%)
  - footer refresh time and Open in {provider}

  Keep the icon retry logic and the loading/none/failed/empty states. Verify with Vitest tests for counts,
  sort changes calling the query, hidden achievements staying hidden, rare label threshold, the button
  hidden for non-Steam, and pt-BR.

## 5. Local save protection (D5)

- [x] 5.1 Storage: the `backup_version_meta` table, written on a successful automatic backup (reason
  after_session, play length from the job's session), Backup now (manual) and the restore's recovery step
  (recovery). Verify with platform/app tests that each path writes the right row, and that a failed backup
  writes none.
- [x] 5.2 Per-installation retention: `BackupSettings.retention` (5/10/20/50, default 10, migration), the
  `--full-limit <retention>` argument in `crates/platform/src/backups.rs`, and the async
  `setLocalBackupRetention` command. Verify with tests: the default argument is 10, the argument follows
  the setting, invalid values are rejected, and another game's setting is unaffected.
- [x] 5.3 `BackupVersion` gains `createdAt`, `reason` and `playSeconds` (joined from meta). The history view
  gains `historyBytes` and `historyCount`. Verify with unit tests for the join (with and without meta) and
  the sums.
- [x] 5.4 TS types and mock host fixtures for 5.1–5.3 (four versions: two after-session, one manual, one
  recovery). Verify with `host.test.ts`.
- [x] 5.5 Rewrite `SaveProtectionModal.tsx` on `DialogShell` (580×700) per `raw/05` and screen D05:
  - status `InsetCard` with `Switch` (`setLocalProfileAutomatic`)
  - save folder field with Change…
  - collapsed "Protected files" disclosure (profile files and registry keys)
  - Keep select
  - Space used
  - history rows with icon per reason, relative day and time, reason line and size
  - Restore
  - footer: Back up now, Open folder, Close

  Keep the restore preview, the overlapping-saves banner and the profile override. Verify with Vitest
  tests: each reason row, unknown origin, a retention change calling the command, space text, restore
  preview still reachable, and pt-BR.

## 6. Game tools (D6)

- [x] 6.1 Rewrite `GameToolsModal.tsx`/`GameToolsPanel.tsx` on `DialogShell` (600×680) per `raw/06` and
  screen D06:
  - tool `InsetCard` with status `Pill`
  - Channel `SegmentedControl`, Version and Proxy `SelectField`s side by side
  - target field with Browse…
  - candidate `Radio` list
  - reset-config checkbox
  - danger `Callout` with the confirmation checkbox (only when anti-cheat is detected)
  - footer download note (size, SHA-256, already downloaded), Cancel, and Install/Update/Uninstall

  Install stays disabled until confirmed. Verify with updated tool modal Vitest tests: disabled state,
  confirm enables Install, candidate selection calls `selectToolTargetExe`, callout absent on clean
  games, footer note variants, and pt-BR.

## 7. DRM-free game settings and custom cover (D7)

- [x] 7.1 Platform: an image file picker (PNG/JPEG/WebP filter), and decode, validate (≤ 20 MB) and re-encode
  to WebP ≤ 600×800 into the cover cache on a worker. Verify with platform tests for valid PNG/JPEG/WebP, a
  corrupt file, an oversize file, and output dimensions.
- [x] 7.2 Core/app: `Registration.custom_cover` (migration), the async `pickLocalCover(entryKey)` returning a
  pending file, commit on `editLocalGame`, clear to the GameNode cover, delete orphans on cancel or replace,
  and `custom_cover` precedence wherever `cover_file` is resolved for local entries. Verify with tests:
  precedence over the GameNode cover, survives a match, cancel deletes the pending file, replace deletes the
  old one, clear restores the GameNode cover.
- [x] 7.3 TS types, commands and mock host (returns a bundled sample cover). Verify with `host.test.ts`.
- [x] 7.4 Rewrite `LocalGameEditor.tsx` on `DialogShell` (640×660) per `raw/07` and screen D07:
  - cover 108×144 with Change cover… and Use the GameNode cover
  - title
  - executable with Locate executable and the several-executables warning
  - folder/working-directory pair
  - arguments
  - "Advanced · process detection" disclosure (collapsed unless it holds custom values) with two textareas
  - footer: Remove from library…, Cancel, Save

  Verify with Vitest tests for the cover pick and clear calls, disclosure default states, the warning
  visibility, Save payload, and pt-BR.

## 8. Discovered games (D8)

- [x] 8.1 Core/app: `Candidate.likelyNonGame` from the shared maintenance-name list plus missing positive
  evidence, and a default `review_selected` that excludes likely non-games. Verify with unit tests:
  `vc_redist.x64.exe` is flagged and unchecked, a recognized game is not flagged, and the list is the same
  one discovery filtering uses.
- [x] 8.2 Core/app: per-candidate suggestion (a queued search per new title, cached by fingerprint, only with a
  verified session). Adding records the suggestion as an automatic match. Verify with tests: one request per
  fingerprint, none offline, an add applies an automatic (not chosen) match.
- [x] 8.3 `reviewSelectedLocalCandidates({ add, allowRediscovery })`: ignore with rediscovery deletes the
  candidate record, otherwise sets `ignored`. Verify with app tests that a rescan re-proposes only the
  rediscoverable one.
- [x] 8.4 TS types, commands and mock host (three rows as in Figma). Verify with `host.test.ts`.
- [x] 8.5 Rewrite `DiscoveryModal.tsx` on `DialogShell` (720×460) per `raw/08` and screen D08:
  - semantic table with a mixed header checkbox
  - cover, title and path
  - compact executable `SelectField`
  - GameNode `Pill` (match, no match, probably not a game)
  - footer: rediscovery checkbox, Ignore selected, Add N games (count follows the selection)

  Verify with Vitest tests for header checkbox states, default unchecked non-games, the Add label count, the
  ignore payload, and pt-BR.

## 9. Confirmations (D9)

- [x] 9.1 Rewrite `RemoveLocalGameModal.tsx` on `ConfirmDialog` (460, danger) per `raw/09` and screen D09:
  two `RadioCard`s, "Remove and don't import again" preselected, store-entry wording, Cancel and Remove.
  Verify with Vitest tests for the default choice, each choice's `allowRediscovery` payload, focus starting
  on Cancel, and pt-BR.
- [x] 9.2 `SettingsModal` exposes `pendingChanges` labels. Rewrite `DiscardChangesDialog.tsx` on
  `ConfirmDialog` (400, warning) with the bullet list, Keep editing and Discard. Update the Portuguese
  scenario text in the settings tests. Verify with Vitest tests: the list shows "Destino dos backups", focus
  starts on Keep editing, and Discard runs the pending action.
- [x] 9.3 Claim conflict: extend `PendingClaim` with `storeEntry` (title, provider, cover, playtime) and
  `registration` (title, root, cover). Rewrite `ClaimConflictModal.tsx` on `ConfirmDialog` (520, warning,
  `IconLink`) with two cards, a chevron, and the actions Cancel, Use existing item and Move tracking. Add the
  mock fixture. Verify with app tests for the populated fields, and Vitest tests for both actions, cancel,
  and a provider icon per provider.
- [x] 9.4 Rewrite `SyncExplanationModal.tsx` on `ConfirmDialog` (480, success, `IconCloudCheck`):
  - "sends" and "stays" `Checklist` per `copy.md`
  - tray note kept
  - Turn sync off runs `setSyncEnabled(false)` then `dismissSyncExplanation`
  - Keep sync on dismisses

  Verify with Vitest tests for the call order, the list content in en and pt-BR, and the tray note
  condition.

## 10. Sign-in (D10)

- [x] 10.1 Core auth: keep `browser_url` in `Pending`, add the `reopenBrowser` input, `Effect::OpenBrowser`,
  `canReopen` and a transient `reopenFailed`. Verify with tests in `auth.rs`: reopen keeps op, nonce and
  expiry; reopen is ignored when not pending; the callback after reopen completes the same attempt; cancel
  after reopen ignores the callback.
- [x] 10.2 Host: handle `OpenBrowser` with the existing browser opener on a worker, report failure, and add the
  async `signInCoverWall()` (up to 25 cached cover file names, most recent first). Add the TS types and mock
  host. Verify with app tests for the opener failure path and the cover-wall limit and order.
- [x] 10.3 Rewrite `SignedOutView.tsx` per `raw/10` and screen D14:
  - art pane with the cover mural (lazy covers, `bg-raised` fallbacks, two overlays, `aria-hidden`, hidden
    below 860px), the headline and the body
  - sign-in column with wordmark, heading, three `ProviderButton`s and the security note
  - version label
  - error/restore states as `Banner`
  - no "Use without an account"

  Verify with Vitest tests: each provider dispatches `startSignIn`, the mural renders the files from
  `signInCoverWall`, the fallback tiles, no "use without account" text, and pt-BR.
- [x] 10.4 Pending state as `ConfirmDialog` (420, accent) per `raw/09` › Entrada em andamento and screen D13:
  waiting vs confirming text, Open again (`reopenBrowser`, disabled when `!canReopen`), Cancel sign-in, and
  Escape cancels. Verify with Vitest tests for each action, the reopen failure message, and focus
  restoration after cancel.

## 11. Integration checks

- [x] 11.1 Run `cargo test --locked -p gamenode-core -p gamenode-api -p gamenode-platform -p gamenode-app`,
  `cargo test --locked -p gamenode-plugin-protocol -p gamenode-plugin-steam`, `pnpm --dir ui test --run`
  and `pnpm --dir ui exec tsc --noEmit`, and verify they all pass.
- [x] 11.2 With `VITE_UI_MOCK=1 pnpm --dir ui dev`, open each of the 14 surfaces and compare it with
  `design-reference/screens/D01…D14`. Check spacing, type, colors, icons and copy in en and pt-BR. Verify by
  recording each surface as checked in the PR description, with any intentional deviation listed in
  `design.md` (five statuses, sync checklist row, remove default, no developer reason).
- [x] 11.3 Check keyboard-only flows across the dialogs (Tab order, Escape, arrow keys in radio groups and
  listboxes, and focus returning to the opener) in the mock. Verify by noting the result in the PR
  description.
- [x] 11.4 Update `docs/development.md` (new mock interactions) and `docs/drm-free-games.md` (custom cover,
  discovery suggestions, rediscovery choice). Verify that `openspec validate redesign-desktop-dialogs
  --strict` still passes.
- [ ] 11.5 **Windows handoff — Lamarck (GameNode Core).** Native Windows acceptance via `scripts/install-windows-wsl.sh`: the cover image picker and
  re-encode, browser reopen during sign-in, Open in Steam, and backup retention with Ludusavi. Verify by
  recording the results in `docs/windows-verification.md`.

### 11.2 completion note — visual check (mock)

Checked with `VITE_UI_MOCK=1 pnpm --dir ui dev` at 1440×900 in English and pt-BR (`&locale=pt-BR`),
against `design-reference/screens/D01…D14` and the matching `raw/*.txt` measurements. Every surface
was opened from its documented mock fixture and compared for spacing, type, colors, icons and copy.

| Surface | Fixture | Result |
|---|---|---|
| D01 Match | `?library=variants` › Neon Courier 2088 › Review… | Matching. No developer-mismatch reason, by design (`design.md` › Intentional deviations). |
| D02 Add | `?library=variants` › Aster Vale › Add to GameNode | Matching. Five status cards by design. The pt-BR `Em andamento` label wrapped instead of clipping after the fix; re-checked. |
| D03 Batch progress | `?library=variants` › three games › Add to GameNode | Matching in the running and finished states, including Hide and the summary. |
| D04 Achievements | `?library=variants` › Aster Vale › See all | Matching: summary, counted segments, sort menu, 65px rows, rare label, footer. |
| D05 Save protection | `?library=variants` › Brasa › Local save protection | Matching. Support and backup-job pills are intentional deviations (`design.md`). |
| D06 Game tools | `?library=variants` › Infinite Coast › Game tools | Matching. License, source, refresh and anti-cheat evidence are intentional deviations (`design.md`). |
| D07 Game settings | `?library=variants` › Brasa › Edit… | Matching: 108×144 cover actions, paths pair, collapsed Advanced disclosure, footer. |
| D08 Discovered games | `?discoveries=1` › Review | Matching: mixed header checkbox, covers, compact select, three pills, rediscovery footer. |
| D09 Remove game | `?library=variants` › Brasa › Remove from library… | Matching; "Remove and don't import again" is preselected by design. |
| D10 Discard changes | Settings › Backups › toggle protection › Escape | Matching: warning tone, listed change, focus on Keep editing. |
| D11 Claim conflict | `?claim=1` | Matching: link tone, two entry cards, chevron, three actions. |
| D12 Sync explanation | `?sync=first` | Matching; the checklist row follows `design.md` (no status/collections row) and keeps the tray note. |
| D13 Sign-in pending | `?scenario=signed-out&signin=pending` › provider | Matching: accent tone, waiting row, Open again and Cancel sign-in. |
| D14 Signed-out | `?scenario=signed-out` | Matching: 5×5 cover mural at the raw positions, wordmark, three providers, security note, version label; no "Use without account". |

### 11.3 completion note — keyboard-only flows (mock)

Exercised in the same mock build after the D01 and D02 fixes; every dialog was reached from the
keyboard only where it opens from a control.

- **Tab order** — D01 search → option → footer actions; D02 close → status radiogroup → New collection → collections → footer; D03 Hide → close; D04 segments → sort → Open in Steam; D05 switch → save folder → Change → Keep → space used → Restore → disclosure → footer; D06 source → channel → version → proxy → refresh → target → candidates → checkboxes → actions; D07 cover → fields → Advanced textareas → footer; D08 header checkbox → row checkboxes → row selects → footer; D09–D13 stay inside the confirmation; D14 providers then window controls.
- **Escape** — closes D01–D08, D10–D12; cancels D09, D13 (and D03 hides without abandoning the run, which finished with its notification).
- **Arrow keys** — D01 listbox moves the selection and the preview (Enter confirms; fixed: an incoming snapshot no longer pulls focus back to the search field); D02, D09 and D06 candidate radios move and select; D04 filter segments and sort menu (Enter, ArrowDown, Enter) work; D06 channel segments change the version; D05 Keep and D08 target selects open and choose with the keyboard; D08 header checkbox cycles mixed/checked/unchecked.
- **Focus return** — D01 Review…, D02 Add to GameNode, D04 See all, D05 Local save protection, D06 Game tools, D07 Edit…, D08 Review, D09 Remove from library…, D10 settings switch, D13 provider button. D11/D12 open from a fixture, so they fall back to the document as expected.
- No gaps left.


## Workflow follow-up

- Archive `add-achievement-tracking` before archiving this change. Its `desktop-achievements` capability
  must exist in `openspec/specs/` first.
- `redesign-desktop-library` should be archived before this change too, so the `desktop-account-library`
  and `desktop-game-matching` main specs already contain its requirements.
