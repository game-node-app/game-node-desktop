# Tasks

- [x] 1.1 Derive the platform filter from the entry's `fields.platform`, normalize it and map
  `gamecube`/`wii` to the catalog abbreviations. Add unit coverage for aliases, unknown/absent
  platforms and empty answers.
  - Verified: `CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-platform-match-filter
    CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 cargo test --locked -p gamenode-core match_dialog`
    → 41 passed, 0 failed, including `the_declared_platform_maps_to_catalog_aliases` (NGC/GC/GCN/
    GameCube case-insensitive, Wii, unknown/blank/absent declarations, disabled filter shows all)
    and `an_entry_without_a_known_platform_is_not_filtered`.
- [x] 1.2 Store the filter in the match dialog, add `SetMatchPlatformFilter(bool)`, filter
  candidates/results and recompute the default selection when it changes. Expose `platformFilter`
  and `filteredOut` in `MatchDialogView`; verify the search request is unchanged.
  - Verified: the same 41-test run, including
    `a_gamecube_entry_shows_the_ngc_candidates_and_results_by_default` (one `SearchGames` with the
    unchanged text query `Pikmin`, `SetMatchPlatformFilter` returns no effects, lists narrow and
    restore) and the serialization test
    `CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-platform-match-filter
    CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 cargo test --locked -p gamenode-core --test
    serialization` → 8 passed, 0 failed (`{ "setMatchPlatformFilter": { "enabled": false } }` and
    the camelCase platform filter payload).
- [x] 1.3 Add core regressions with a GameCube entry returning NGC and Switch options (NGC visible
  by default, Switch after show-all), a Wii entry, no-platform entry and a fully filtered answer.
  - Verified: `CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-platform-match-filter
    CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 cargo test --locked -p gamenode-core` → 679 passed,
    0 failed, including `a_gamecube_entry_shows_the_ngc_candidates_and_results_by_default`,
    `a_wii_entry_matches_the_wii_alias_and_keeps_its_manual_choice`,
    `an_entry_without_a_known_platform_is_not_filtered`,
    `an_answer_the_filter_hides_entirely_is_not_a_failed_search` and
    `toggling_the_filter_recomputes_a_hidden_selection`.
- [x] 1.4 Add the dialog chip/switch and “Show all” action, the filtered-empty notice, en/pt-BR
  labels and a Pikmin/NGC mock fixture. Verify keyboard/focus behavior is unchanged.
  - Verified: `npx --yes pnpm@10 --dir ui test --run src/features/dialogs/dialogs.test.tsx` → 23
    passed, 0 failed (platform switch, Show all, filtered-empty notice, no-platform entry and the
    existing Enter/double-click/arrow-key/focus tests); `npx --yes pnpm@10 --dir ui test --run
    src/i18n/i18n.test.ts` → 11 passed, 0 failed (en/pt-BR key and placeholder parity); the mock
    fixture is `dolphin:pikmin`/`dolphin:metroid` in `?library=variants`.
- [x] 1.5 Add UI tests for default filtering, toggling off, empty-filtered state and an entry with
  no platform.
  - Verified: `npx --yes pnpm@10 --dir ui test --run` → 134 files passed, 1335 tests passed, 0
    failed, including the new dialog tests and
    `src/mock/host.test.ts › narrows the Dolphin match dialog by the declared platform and restores
    it on Show all` (GameCube NGC-only, Show all adds Switch, Wii and no-platform rows).
- [ ] 1.6 Run the final focused checks and confirm the Pikmin GameCube vs Switch case in the
  rebuilt desktop before marking this task complete.
  - Automated evidence: `npx --yes pnpm@10 --dir ui exec tsc -b` → clean; `npx --yes pnpm@10
    --dir ui build` → built in 6.36 s; `cargo fmt --all -- --check` → clean; `git diff --check` →
    clean; `openspec validate --strict add-platform-aware-match-filter` → valid;
    `CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-platform-match-filter
    CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0 cargo clippy --locked -p gamenode-core
    --all-targets` → clean.
  - The Pikmin GameCube vs Switch case is covered by the mock-host regression above (NGC-only
    candidates/results, then Switch after Show all). Native interactive confirmation and the
    desktop rebuild are still pending, so this task stays open.

## Out of Scope

- Server-side platform filters, confidence changes, automatic suggestion ranking and a persisted
  user preference. These remain follow-ups.
