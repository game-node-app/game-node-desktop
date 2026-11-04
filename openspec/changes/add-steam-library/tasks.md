# Tasks

Prerequisite: finish and archive `bootstrap-desktop-foundation` before applying this change, so that `desktop-shell`, `desktop-auth` and `desktop-local-state` exist in `openspec/specs/` for the modified requirements below.

## 1. Dependencies and scaffolding

- [ ] 1.1 Add `serde` (derive) and `toml` to `gamenode-core`, the `Win32_System_Registry` feature to `gamenode-platform`'s `windows-sys`, and Slint's JPEG image decoding feature to `gamenode-app`; add an empty `providers/` directory at the repository root; verify `cargo check --workspace --locked` passes on Linux and the Windows cross-build, and `Cargo.lock` is committed.
- [ ] 1.2 Create the `core::provider` module tree (`manifest`, `registry`, `record`, `template`, `steam`) and `core::library` with module docs, and update `core/src/lib.rs` docs that said provider interfaces arrive later; verify `cargo clippy --workspace --all-targets --locked -- -D warnings` passes and `core` still has no OS, UI or network dependency in `cargo tree -p gamenode-core`.

## 2. Provider runtime

- [ ] 2.1 Add the `HostFiles` and `RootResolver` ports to `core::ports` and an in-memory `HostFiles` test double with Windows- and Unix-style paths; verify unit tests read files, list directories and canonicalize a linked path through the double.
- [ ] 2.2 Implement manifest types with `deny_unknown_fields` (id, name, platforms, source, scan, launch, install, match, exclude_ids, roots per platform) and a loader over `(file name, text)` pairs that skips manifests for other platforms; verify tests load the documented Steam example and skip a Linux-only manifest on Windows.
- [ ] 2.3 Implement validation that collects one diagnostic per invalid manifest (unknown field, missing field, unknown native scanner, duplicate id, malformed placeholder, non-URI or placeholder-in-scheme template) and keeps loading the rest; verify a table-driven test covers each case and that a valid manifest still loads alongside an invalid one.
- [ ] 2.4 Implement the record type and the scan step that drops records without a usable `id` or `install_dir` with a log; verify tests for an incomplete record and that other records survive.
- [ ] 2.5 Implement template filling: `{[a-z_]+}` placeholders, path-segment percent-encoding, rejection of control characters, missing field makes the action unavailable, and a scheme check of the filled URI against the template's literal scheme; verify tests for `steam://rungameid/620`, values containing `/ ? # % space` and control characters, and a missing field.
- [ ] 2.6 Generate `include_str!` entries for `providers/*.toml` in `crates/app/build.rs` and pass them to the loader at startup, logging diagnostics; verify a build test embeds every file in `providers/` and that the app reads no manifest from disk at runtime.
- [ ] 2.7 Write `docs/providers.md` describing the step 2 manifest schema, placeholder rules, URI-only launch and the exclude list; verify the documented Steam example parses in a core test that loads it from the doc's text.

## 3. Steam scanner

- [ ] 3.1 Write the KeyValues parser in `core::provider::steam` (quoted and bare tokens, escapes, nested blocks, `//` comments, `[$COND]` tags) failing per file; verify table-driven tests including malformed input.
- [ ] 3.2 Implement the Steam scanner: canonicalize and deduplicate roots, read `steamapps/libraryfolders.vdf` (falling back to `config/libraryfolders.vdf`) in the current and old formats, always include the root, read every `appmanifest_*.acf`, keep numeric app ids whose `steamapps/common/<installdir>` exists, normalize Windows slashes, and drop `exclude_ids`; verify fixture tests using anonymized real files for Windows with two libraries, native Linux, Flatpak, a symlinked `~/.steam/steam`, the old `libraryfolders` format, an offline library folder, a malformed manifest and a tool app.
- [ ] 3.3 Add `providers/steam.toml` with Windows registry and Linux native and Flatpak roots, `steam://` launch and install templates, `external_category = 1`, and the tool, runtime, Proton and redistributable exclude list; verify the loader accepts it and a fixture containing app 228980, a Steam Linux Runtime and a Proton build yields no tool entries.
- [ ] 3.4 Implement `HostFiles` with `std::fs` and `RootResolver` in `gamenode-platform` (home expansion, and `HKCU` string values via `windows-sys` on Windows); verify a Linux temp-directory test resolves `~` paths and a Windows test reads a value written under a test `HKCU` key and removes it afterwards.

## 4. Local storage

- [ ] 4.1 Add migration `0002_library.sql` with `library_entries`, `provider_scans`, `auto_matches`, `chosen_matches` and `games`, registered in `MIGRATIONS`; verify the existing migration tests plus a step 1 fixture upgrade that keeps settings, and that a step 1 build's newer-schema check would leave the v2 database untouched.
- [ ] 4.2 Implement repository methods for loading the library cache, saving a provider's scan result as one transaction, reading and writing automatic and per-account chosen matches, and game metadata and cover state; verify temp-database tests for a rescan, an uninstall that keeps the match, a failed provider scan, and two accounts with different chosen matches.
- [ ] 4.3 Store `library.layout` and `account.last_id`/`account.last_name` settings and remove the account keys on sign-out; verify tests that layout survives reopening, sign-out removes the account keys, and no fixture contains a credential or callback URL.
- [ ] 4.4 Update `docs/local-state.md` with the new tables, settings keys, the `covers/` folder and what sign-out keeps; verify the documented paths and keys match the migration and the platform path functions.

## 5. Library state in core

- [ ] 5.1 Implement `LibraryMachine` inputs and effects for loading the cache, scan requests with one running scan and a single pending flag, and per-provider merge where failures keep the previous set and entries are never deleted; verify transition tests for repeated rescan requests, an uninstall, a reinstall that keeps match and cover, a moved install folder, and one failed provider.
- [ ] 5.2 Implement pure filter (all or one provider), case-insensitive search on detected and matched titles, stable sort by title, provider or match status in both directions, and per-provider counts; verify unit tests including "nothing matches" and title tie-breaks.
- [ ] 5.3 Implement selection with an anchor (click, Ctrl-toggle, Shift-range, select by keyboard) and grid and list movement (arrows by column count, Home/End); verify unit tests for each mode and that selection survives a view switch and a filter change.
- [ ] 5.4 Implement action availability per entry and per multi-selection (Play, Install, Match/Change match, Add to GameNode) with disabled reasons, including the offline and unverified reasons; verify tests for an unmatched game, a multi-selection without single-game actions, and a cached library before verification.
- [ ] 5.5 Add `library: Option<LibraryView>` to `Snapshot` with `Arc`-shared rows and a generation number, plus layout state and `SetView`/`SetFilter`/`SetSearch`/`SetSort`/`Select`/`Move` commands; verify tests that unchanged state keeps the generation and that search text is not part of the persisted layout.

## 6. Matching

- [ ] 6.1 Add a `GameApi` port and implement `lookup_external` in `gamenode-api` with batches of 50 uids, `limit=100`, paging while `hasNextPage`, and rows deduplicated by `gameId`; verify mock-server tests for 120 ids across requests, one uid with two games, a 401 classified as a rejected session, and a timeout.
- [ ] 6.2 Implement `search_games` against `POST /v1/search/games` returning id, name, release year and cover URL; verify mock-server tests for results, an empty query result and a failure.
- [ ] 6.3 Implement match precedence in core (chosen game or "no game", then a single automatic match, else unmatched with candidates), lookups only for verified sessions after scans that add ids or for ids checked more than 24 hours ago, and failed lookups that change nothing; verify transition tests for an ambiguous id, a manual pick surviving a different lookup result, a sticky "no game", a failed lookup, and no lookup while unverified.
- [ ] 6.4 Implement the match dialog state (prefilled query, ambiguous candidates first, edit and pick, clear match, mark as no GameNode game, search error that keeps the entry's state) and show automatic versus chosen in the detail view; verify core tests for each path.

## 7. Covers

- [ ] 7.1 Implement the cover URL check (normalize `//` to `https:`, host `images.igdb.com`, IGDB upload path pattern, rewrite size to `t_cover_big`); verify unit tests for allowed, http, wrong host, wrong path and malformed URLs.
- [ ] 7.2 Implement `fetch_cover` with at most 4 concurrent downloads and a 5 MB response cap, atomic writes to `<data>/covers/<game_id>.jpg`, `cover_failed_at` on failure, and retry on the next scan; verify mock-server and temp-directory tests for success, an oversized response, a failure then retry, and a match change pointing at another file.

## 8. Add to GameNode

- [ ] 8.1 Implement `library_entry` (404 is absent), `list_collections` and `create_entry` (platform `[6]`, `finishedAt: null`) in `gamenode-api`; verify mock-server tests check request bodies and the 404 and error cases.
- [ ] 8.2 Implement membership loading on selection and the Add dialog state (status picker, the user's collections, a selected collection's default status shown as the status that applies, errors that keep choices); verify core tests for an entry already in the library, membership that cannot be read, and a collection with a default status.
- [ ] 8.3 Implement the confirm flow that re-reads membership per game and creates only absent entries, one game at a time, with an added, already present, skipped-unmatched and failed summary; verify tests where a game is added elsewhere before confirm (no create request is sent) and the five-game mixed selection from the spec.
- [ ] 8.4 Verify that scanning, matching, launching and signing in never call `create_entry`, with a test that runs a full scan, lookup and sign-in against a mock server that fails on any create request.

## 9. Engine integration

- [ ] 9.1 Add a `UriOpener` port and platform implementation that passes one validated URI to the OS handler (the `open` crate on Windows, `xdg-open` on Linux) without a shell, kept separate from `BrowserOpener`; verify tests with an injected opener for Play, Install and a missing handler error, and that `BrowserOpener` still accepts only http(s).
- [ ] 9.2 Wire `LibraryMachine` into the `Engine`: scans in `spawn_blocking` at startup after the cache loads, on window show and second-launch activation, and on Rescan; persistence on the FIFO storage worker; lookups and covers on Tokio only while verified; verify engine tests with delayed mock scanners and APIs that the snapshot stays responsive and requests coalesce.
- [ ] 9.3 Integrate auth: store the last account on `SignedIn`, open that account's cache while `Verifying` or `RestoreFailed` with network actions disabled, enable them when the same account verifies, reload for a different account, and clear the library and account keys on rejection or sign-out; verify engine tests for each case from the `desktop-auth` delta.
- [ ] 9.4 Update `docs/development.md` with the fixture layout for scanner tests and how to run the app against a local Steam fixture; verify the documented commands run as written.

## 10. Library UI

- [ ] 10.1 Build the signed-in library layout in Slint: top bar with search, view switch, sort and Rescan; sidebar with All games and providers with counts; content area; resizable detail pane; scan status; app menu Rescan; verify by running the app against fixtures that every control is reachable by keyboard and focus is always visible.
- [ ] 10.2 Build the grid (row-virtualized `ListView` with covers per row computed from width) and the list (title, provider, match status), with cover or titled placeholder; verify against a fixture library that switching views keeps entries, filter, sort and selection.
- [ ] 10.3 Forward keyboard handling (arrows, Home/End, Enter, Ctrl+Enter, Ctrl/Shift selection, Ctrl+F, F5, Menu or Shift+F10) and mouse selection to core commands, and add context menus with `ContextMenuArea` for single and multi-selection; verify manually in both views and with core tests for the forwarded commands.
- [ ] 10.4 Build the detail pane (cover, detected and matched titles, provider, install folder, match status and source, membership, actions with disabled reasons, Play progress and errors) and the empty-library and nothing-matches states; verify each spec scenario against fixtures.
- [ ] 10.5 Build the Match and Add to GameNode overlay dialogs with focus trapping and Escape to cancel, reusing the step 1 overlay pattern, and the multi-add summary; verify both dialogs by keyboard only against a mock server.
- [ ] 10.6 Save and restore the layout (view, sidebar selection, sort, pane sizes, detail open) and leave search empty on start; verify by changing each setting, restarting and checking the restored layout.
- [ ] 10.7 Add every new UI and Rust-originated string to the English and Brazilian Portuguese catalogs under `i18n/`, showing game titles and collection names as received; verify catalog compilation, no missing keys on every library screen, and a pt-BR run that shows translated chrome and unchanged titles.

## 11. Roadmap acceptance

- [ ] 11.1 On a real Windows PC with Steam games in two library folders, install the build, sign in, and confirm the library lists the installed games with matches and covers, Play starts a game through Steam, Install opens Steam, Rescan picks up an uninstall, a manual match sticks, and Add to GameNode adds one game and a multi-selection without changing an existing entry; record version, revision and results.
- [ ] 11.2 Repeat 11.1 on Linux with native and Flatpak Steam and Proton installed, confirming no Proton, runtime or redistributable entries appear and that each game appears once; record results.
- [ ] 11.3 On either OS, sign in, quit, disconnect the network and restart, and confirm the cached library, covers, Play and Install work, matching and Add are disabled with an explanation, and reconnecting and retrying enables them; then sign out and confirm sign-in is shown with no library; record results.
- [ ] 11.4 Audit the shipped scope against the roadmap: no executable launches, user manifests, generic scanners, playtime, tracking, reporting of detections or edits to existing GameNode entries; record the result.

## Workflow follow-up

- Archive `add-steam-library` after review, once `bootstrap-desktop-foundation` has been archived.
- Verify the archived specs include `desktop-providers`, `desktop-library`, `desktop-game-matching` and `desktop-account-library`, and the modified `desktop-shell`, `desktop-auth` and `desktop-local-state` requirements.
