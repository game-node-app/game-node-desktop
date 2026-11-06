# Design

## Context

See `proposal.md` for motivation and the delta specs for acceptance behavior. Discovery used the current implementation, not only the roadmap:

- Only Steam is bundled. `provider::Manifest` accepts native scans and URI actions; `library::launch::open_action` only opens URIs. `Provider::external_category` drives automatic matches, while `match_dialog` already preserves account-specific choices and explicit no-match.
- `GameApi` supports external lookups and title search. `crates/api/src/game.rs` requests at most 20 search results and currently drops pagination information. Name inference and automatic name matching do not exist.
- `Tracker` receives `{pid, created_at, exe_path}`, matches by deepest install folder, waits for exits and writes checkpointed sessions. Its `CloseSession` also occurs on account changes, Stop and 24-hour splits. It is therefore unsuitable as a backup trigger by itself.
- `engine/playtime.rs` lists processes on background workers every five seconds, publishes snapshots and uses a FIFO storage worker. Linux tracking returns Unsupported. There is no executable metadata reader, direct launcher, directory watcher or backup engine adapter.
- Manual installation records and backup tables do not exist. The latest migration is `0003_playtime.sql`. Windows packaging explicitly enumerates files and must be extended to carry the helper and manifest.
- `add-playtime-background` is still in flight, including background/tray acceptance. Its `desktop-playtime` spec is not in main specs yet. This design depends on its completion and spec synchronization/archive. The delta for that capability explicitly targets that future baseline; do not apply/archive the changes in reverse order.

## Goals / Non-Goals

**Goals:** Keep a one-picker fast path, represent uncertainty without blocking play, reuse one tracker for all launch sources, and protect saves without coupling them to catalog edits or network availability. Keep core free of OS, subprocess and filesystem calls; all blocking work belongs on workers and results return as bounded events.

**Non-Goals:** Store integrations, auto-importing arbitrary executables, whole-disk scanning, cloud saves, automatic restore, periodic backups while running, Linux adapters, or arbitrary downloaded launch scripts. Save protection initially applies to DRM-free registrations; the adapter can serve store games in later work without making that UI part of this change.

## Decisions

### 1. Stable manual registrations and a short add flow

Add a bundled Windows `manual` provider, displayed as DRM-free, with native scanner `manual`, source `manual`, no counter and no Install. Extend the launch declaration to a tagged action (`uri` or the built-in manual executable action). Existing URI declarations retain their parsing and validation. The manual scanner projects persisted registrations into the existing plain `Record` shape; it never guesses new registrations from the disk.

Add game opens a native executable picker on an appropriate background/UI-dialog adapter. Selection validates and persists a UUID registration, selects the entry and returns control. It does not wait for GameNode or a full manifest scan. Persist the path, title, inferred root, working directory and argument vector; default to the selected executable's directory and no arguments unless positive install-root evidence improves the root. Show advanced settings after adding, including an argument editor with explicit Windows argument parsing and validation. Recognized bootstrap/launcher executables get a role distinct from gameplay.

Identity is the UUID, not a path hash, title or manifest id. Canonical Windows paths/file identities deduplicate re-adds, including junction aliases. Locate executable changes the registration in place. Missing manual executables stay visible and cannot be launched; scan absence retains store uninstall behavior. Removing a registration tombstones it and its discovery identity without deleting files or archives. Explicit Add game can override a tombstone.

Alternative: make GameNode selection part of an add wizard. Rejected because it makes play depend on catalog coverage and connectivity. A path-derived id was rejected because moves would lose history.

### 2. Evidence pipeline with two separate matching results

Collect evidence on workers: PE `ProductName`, `FileDescription` and original filename; meaningful executable basename; game-folder segments; primary-manifest canonical titles, aliases, `installDir` hints and applicable `launch` executable hints. Product metadata can be absent, generic or misleading, so retain provenance and contradictory signals. Do not execute a file to identify it.

Use explicit confidence rules rather than a fuzzy score cutoff:

1. A user-selected executable establishes an intentional installation, but not a catalog identity. Watched discovery additionally requires positive game evidence: for example a known manifest launch path plus installation context, or a manifest installation/title identity corroborated by meaningful game metadata. Folder-name similarity alone is insufficient. Exclude setup/uninstall/update/crash/redist/support binaries before candidate selection.
2. Prefer meaningful product metadata, corroborated by executable/folder evidence. Otherwise try a meaningful executable name, then evidenced folder/manifest identity. Generic engine/launcher names cannot supply a strong title. Preserve sequel numbers, Roman numerals, remaster/edition words and platform distinctions. Normalize case, Unicode representation, spacing and equivalent punctuation only.
3. If a strongly identified manifest profile supplies an external id, reuse external lookup (Steam category 1, GOG category 5) to establish GameNode identity. These are identity hints only: the entry remains provider/source `manual`.
4. Otherwise search the meaningful title and accept only one conservative exact identity with no conflicting evidence. Preserve search completeness information in the API result; a full 20-result page or unknown/truncated result is insufficient to prove uniqueness and yields suggestions. Never select the first result merely because the endpoint sorts it first.

Keep the original detected title and automatic provenance beside `GameRecord`. Cache name-based results by installation/evidence revision instead of pretending they are provider ids in `auto_matches`. Existing account-specific chosen/no-match always wins. Operations carry entry/evidence/account generations, so repair, removal and sign-out invalidate old responses. Retry offline lookups after verified access returns, using the current lookup cache age and bounded concurrency.

Resolve a **save profile** separately against the local manifest index. A strong canonical identity can support saves before catalog matching completes. Multiple plausible profiles require selection. Catalog corrections can suggest a different save profile but never silently migrate backups. Users can pick a different profile, disable it, or select explicit save files/folders for unsupported games. Custom file selection creates a local profile without asking users to author manifests.

Alternative: use fuzzy matching or Ludusavi's first `find` result as proof. Rejected because titles/editions can collide and `find` defaults to stopping at the first match. Manifest launch data is optional; its absence does not imply a game is unsupported.

### 3. Watched folders produce a review queue

The user selected review before import. A Windows folder-watch adapter reports changes; a background reconciliation pass runs on startup and explicit rescan and after watcher overflow/reconnect. Keep events bounded, coalesce per-root requests with a one-second debounce, and never repeatedly scan the whole library for every changed file. One active discovery scan and one coalesced follow-up are enough.

Default discovery examines root child directories as installation candidates, with bounded traversal (eight levels and 10,000 entries per installation); stop and report incomplete discovery at the cap. Skip reparse points escaping selected roots and deduplicate overlapping roots/canonical file identities. A long or incomplete copy is reconsidered after settling, not silently finalized. Unknown folders stay omitted and can still be added by executable selection.

One candidate represents one installation. Multiple plausible executables are a launch-choice ambiguity, not several games. Review shows the title, root, target, evidence and issues, and supports multi-select Add and Ignore. Add is enabled only with a resolved valid launch target. No discovery item is launched, tracked, backed up, catalog-reported as ownership or added to the GameNode collection before approval. Persist ignores; a changed game executable can trigger reconsideration, but incidental files must not repeatedly resurrect ignored entries. Removing a watch stops discovery and retains approved entries.

Alternative: automatically add high-confidence candidates. Not selected by the user. No metadata source can guarantee that an arbitrary executable is a game.

### 4. Explicit process policies and physical play periods

Extend tracker `Entry` with a policy: existing store folder matching, or DRM-free gameplay executables plus optional launcher bindings. Windows metadata/process adapters supply parent identity when observable, without making PID alone an ancestry proof. Extend platform ports; other platforms return Unsupported. Exact canonical executable bindings outrank folder fallback, then deepest roots and a stable tie-break decide ownership. An existing store claim is surfaced when adding the same executable manually so the user can use that entry or explicitly transfer ownership; it must never count twice.

For manual games the selected ordinary executable is the default gameplay binding. Known launchers are start targets only. Adopt a game child only when its path is inside the registered root and it is a configured or positively identified gameplay executable; ancestry alone is insufficient. Persist positively established additional gameplay bindings so external starts and a GameNode restart can recognize them later. Do not adopt a shared engine outside the root implicitly; expose a configuration limitation and allow choosing an appropriate game root/target.

Keep `(pid, created_at)` identity and native exit waits. Use a ten-second bounded handoff window for bootstrap/replacement gameplay processes. Launcher time does not become gameplay time. Once gameplay has been observed, replacements within that window keep one physical play period; retain the last actual gameplay exit timestamp so the wait adds no playtime. Persistent launcher, updater and crash-reporter processes cannot hold that period open. Short games that start and exit entirely between five-second listings may be missed; a direct-start handle improves observation for GameNode launches, but background process tracing is not promised here.

Add tracker output for `GameStopped { installation, play_period_id, last_gameplay_exit, reason }` independently of measured-session close. Administrative closes, source switches and 24-hour splits preserve or end tracking state as required by step 3, but never synthesize a physical-exit event. Exit callbacks and successful listings converge on one stopped transition. Enumeration errors preserve prior state. Restored checkpoints do not imply physical exit; re-enumerate before backup recovery.

Alternative: back up on every `CloseSession`, or reuse folder-only tracking for manual games. Rejected because both create backups at wrong times and can count maintenance tools as gameplay.

### 5. Ship a compatible native helper and isolate it

Bundle Ludusavi **v0.31.0 Windows x64** as a sidecar and a pinned baseline primary manifest in Windows packages. The upstream release currently publishes `ludusavi-v0.31.0-win64.zip` with SHA-256 `f47a8ad8c708f01d2eb124704973beffab205e292f5287a10fc4a101f8d68706`. Package preparation downloads from that fixed official release, verifies the pinned digest and records executable/manifest digests and source revisions in build metadata. Pin the manifest revision/hash during implementation; this is release input selection, not a runtime latest-release lookup. Include upstream notices and data provenance in the existing licenses/About flow. The Windows installer copies the sidecar and the uninstaller preserves user backups. Development builds may report backup unavailable until the package assets are prepared.

No runtime executable download, PATH lookup or binary auto-update. Helper updates ship with GameNode; metadata updates are data only. This intentionally extends the roadmap's one-binary/no-downloaded-code description with a packaged, fixed-purpose native helper. The app still owns the only persistent process; CLI invocations are short-lived jobs.

Put a `BackupEngine` port in core and implement it in platform. Use absolute helper paths, argument vectors/stdin, no shell, no GUI flags, an isolated `--config` directory, `--no-manifest-update`, and `--no-cloud-sync` for backup/restore. Always pass exactly one canonical title after `--` (or bounded stdin); empty game selection is an adapter error. Check helper version and contract once, and parse both exit status and per-game/file/registry results. Bound output (8 MiB), time (five minutes by default), cancellation and child cleanup; report partial operations and reconcile interrupted writes. Never hold the engine state lock or occupy UI callbacks while doing helper, path, YAML, database, image or network work.

Alternative: ask users to install/configure Ludusavi, or download it on first backup. Bundling gives deterministic offline behavior and avoids a setup branch. Reimplementing its backup engine would duplicate save/registry/retention handling and is out of scope.

### 6. Cache the manifest and bind saves to the actual installation

Maintain a bounded tolerant index of the primary manifest for identity hints. Cache under GameNode app data with source revision, hash, ETag and last-check metadata. At most once per day, update from the fixed official raw HTTPS manifest URL outside play/backup; cap download size (32 MiB), validate structure and bounded aliases before atomic activation, and keep the last known good copy. Unknown fields are tolerated per the upstream general schema. Pin a snapshot reference for each job and never change its rules mid-operation.

For helper execution, generate a **per-profile private primary manifest** containing the selected resolved definition, preserving file/registry rules, tags and applicable constraints. Bind `<base>`, `<root>` and `<game>` to the selected installation's concrete context with correct glob escaping; leave OS/user placeholder expansion and glob evaluation to Ludusavi. Remove launch/install discovery hints from this execution manifest. Set helper roots and secondary-manifest lists to empty: the helper must not auto-discover `.ludusavi.yaml` in game folders or unrelated installed copies. This small binding step is necessary when a DRM-free installation has a renamed folder or nested executable; `--path` selects the backup destination, not the install directory.

Test the generated config against the pinned helper, especially renamed roots, spaces/glob metacharacters, appdata/document saves, store-constrained rules and registry keys. Unsupported unresolved placeholders or ambiguous users/paths produce needs-selection/unsupported rather than guessing a global folder. Before activating an inferred or custom profile, run a scoped preview. Show locations and allow correction; no saves found is a normal supported state before first play. Game-folder secondary manifests are hints for an explicit reviewed local definition only, not implicitly merged authority.

Alternative: register the user's whole NonSteamLibrary as a generic Ludusavi root. Rejected because title/folder variations and multiple installed copies could back up the wrong data or ingest local secondary manifests. GameNode does not implement its own save-path database or full manifest evaluator.

### 7. Durable backups after play, explicit restore

Use one helper writer at a time across backup, restore and manifest/config mutations. Local automatic backup defaults on for resolved supported DRM-free profiles, with a global switch and per-installation override. The default path is `%LOCALAPPDATA%\GameNode\Desktop\backups`; explain that this protects against save corruption/accidental overwrite, not loss of the same physical drive. Users can choose another local destination. Changing it affects new backups and keeps old destinations discoverable; it does not silently move/delete archives.

Namespace archives as `backups/manual/<installation uuid>/<profile id>/`, independent of catalog matches. A profile id changes on explicit retargeting; rules/config revisions are frozen in job metadata. Two installations can legitimately share an OS-level save location; detect overlapping previews, warn in details and serialize operations by overlapping resources. Distinct archive namespaces do not imply distinct live saves. Restore previews always use the selected archive's recorded targets.

After a physical stop, enqueue one job keyed by installation, profile revision and play-period id; wait three seconds beyond the handoff settlement and perform a fresh successful process check. If gameplay resumed, coalesce/defer to its next physical stop. Backup now uses the same gate. If the process check is uncertain, wait rather than assuming stopped. Mark durable job state before starting the child; capture engine backup ids and reconcile actual archive history after interruption. Transient failures retry at 30 seconds, two minutes and ten minutes, then wait for explicit retry or the next exit. Global/per-game disable cancels queued automatic jobs; manual jobs require their own user action.

Use ten full changed backups and zero differentials by default for simple independent restore points. Map unchanged/partial/failed outcomes explicitly; only successful committed backups permit pruning. Preserve recovery snapshots outside ordinary retention, including mapping/metadata, until the user explicitly removes them through a later management capability. Expose disk usage and failures; do not claim backup protects against device loss or completed after-play protection while the helper is pending.

Run mutating helper backups against a staged copy of the profile namespace, so helper retention or partial writes cannot prune the active usable history. Prefer filesystem copy-on-write cloning where supported and ordinary background copies otherwise; never hard-link mutable mapping files. Validate the completed report and archive, then promote the staged namespace under an operation journal with recovery for interruption between directory renames. A failed/partial attempt is recorded but does not replace active history. Recovery backups use a separate namespace and the same verified promotion rule. Stage space is included in destination capacity checks and cleaned after reconciliation.

Restore is never part of Play. Select a historical backup, obtain a read-only preview, show replacements, confirm, and check stopped state again before writes. Create and verify a recovery backup of the **current data at all affected restore targets**, including registry content, then restore the exact backup id with no cloud sync. If normal profile rules no longer cover all targets, use a temporary scoped recovery definition from the preview. Failure to preserve existing targets aborts restore. No present data is a valid empty recovery baseline. Keep unrelated files untouched and report partial restore with a path to recovery. An external game can still start after the final check; monitor during work, cancel when observed and retain recovery history. Full atomic coordination with external launchers is not possible and must not be claimed.

Alternative: use `ludusavi wrap`. It only covers GameNode launches and encourages pre-launch restore; the existing background tracker is the correct source for after-play protection.

### 8. Persistence and UI boundaries

Add the next migration after the prerequisite's current schema (expected `0004_drm_free_backups.sql`). Model registrations/tombstones, watched roots, candidates/ignores, inferred match provenance, save profiles/revisions and jobs/results in dedicated tables. Extend the settings allowlist for backup control/destination. Keep chosen GameNode matches per account; local registrations, save profiles, archives and protection preferences belong to the Windows user, consistent with the live saves. Account changes cancel stale catalog work; they do not silently retarget OS-local backup profiles.

UI snapshots carry add/discovery progress, candidate rows, match provenance, executable settings, save status/history and restore preview. Commands are short and asynchronous; file pickers, validation, scan, decode, archive I/O and waits run through adapters/workers. Integrate in the existing library/detail overlays and Settings rather than adding a mandatory onboarding wizard. Add keyboard focus/escape behavior and all strings to English and `pt_BR`. Linux builds remain working with new Windows features unavailable.

## Risks / Trade-offs

- [Heuristics miss games or fail to identify a launcher] → Keep Add game unconditional for deliberate executable selection, conservative reviewed candidates and editable gameplay bindings; document inspection/elevation and five-second sampling limits.
- [Incorrect save metadata protects unrelated files] → Preview, separate profile identity, scoped execution manifests, location correction, strict target/destination overlap checks and explicit restore. Support improves through upstream metadata, not silent widening.
- [Slow/full/offline backup destination] → Bounded worker jobs, visible failure/retry, preserved previous archives and no launch dependency. Reject destinations overlapping watched installs or save sources, including canonical aliases.
- [Recovery snapshots consume storage] → Show their size and preserve recoverability; automatic cleanup of recovery archives is deliberately excluded from ordinary ten-version retention.
- [Process exits between probes or protected processes are unreadable] → Never invent gameplay or claim an uncertain stopped state. Windows acceptance must include actual multi-process games; unsupported cases remain visible limitations.
- [Native sidecar/data binding expands packaging and trust boundaries] → Pin/hash release inputs, isolate config, disable implicit network/cloud work and local secondary discovery, include notices and use compatibility fixtures.
- [Concurrent external writers defeat an application-level restore lock] → Stopped checks, monitoring, explicit confirmation and verified recovery are best effort; partial restores remain reported as such.

## Migration Plan

1. Complete and sync/archive `add-playtime-background`; confirm its final process port, migration registry and playtime requirement are the expected baseline. Preserve its existing tasks/artifacts while planning this change.
2. Implement new data tables transactionally without rewriting Steam rows, playtime or user matches. Rollback to an older binary uses existing newer-schema refusal; preserve the data directory and archives rather than resetting it.
3. Ship the pinned sidecar, baseline manifest and notices with the Windows package. No existing user is forced through a new add/backup setup. The backup default applies to new supported DRM-free profiles, not existing Steam entries.
4. Update the roadmap as step 4a DRM-free/local backups and step 4b Epic/GOG/Ubisoft. Generic store scanners move to 4b; steps 5/7 keep their appropriate dependency on the store portion. Document the packaged helper and save/registry restore write exception to the old OptiScaler-only rule.
5. Verify Windows behavior with fixture executables plus real DRM-free games, including external launch, persistent launcher, removable drive and isolated file/registry restore. Keep Linux compile/unit checks; real Linux desktop feature verification remains its own milestone.

## References

Primary sources checked while planning (2026-11-05); pin integration fixtures to the chosen release rather than assuming future master is compatible:

- [Ludusavi v0.31.0 release](https://github.com/mtkennerly/ludusavi/releases/tag/v0.31.0) and [versioned CLI contract](https://github.com/mtkennerly/ludusavi/blob/v0.31.0/docs/cli.md): isolated config, scoped names, preview, JSON, selected restore ids, retention and no-cloud flags.
- [Versioned config schema](https://github.com/mtkennerly/ludusavi/blob/v0.31.0/docs/schema/config.yaml) and [JSON output guidance](https://github.com/mtkennerly/ludusavi/blob/v0.31.0/docs/help/command-line.md): custom profiles, roots and failure interpretation.
- [Manifest format](https://github.com/mtkennerly/ludusavi-manifest) and [general schema](https://github.com/mtkennerly/ludusavi-manifest/blob/master/data/schema.yaml): optional launch hints, install folders, ids, aliases, placeholders and forward-compatible fields; none guarantees a folder contains an actual game.
- [Ludusavi license](https://github.com/mtkennerly/ludusavi/blob/v0.31.0/LICENSE) and [manifest license](https://github.com/mtkennerly/ludusavi-manifest/blob/master/LICENSE): preserve upstream notices and dataset provenance in the distribution.
- [Ludusavi Playnite integration](https://github.com/mtkennerly/ludusavi-playnite): existing precedent for after-play backups; its separate-install/root setup is not the proposed GameNode UX.
