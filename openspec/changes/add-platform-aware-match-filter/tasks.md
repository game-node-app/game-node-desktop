# Tasks

- [ ] 1.1 Derive the platform filter from the entry's `fields.platform`, normalize it and map
  `gamecube`/`wii` to the catalog abbreviations. Add unit coverage for aliases, unknown/absent
  platforms and empty answers.
- [ ] 1.2 Store the filter in the match dialog, add `SetMatchPlatformFilter(bool)`, filter
  candidates/results and recompute the default selection when it changes. Expose `platformFilter`
  and `filteredOut` in `MatchDialogView`; verify the search request is unchanged.
- [ ] 1.3 Add core regressions with a GameCube entry returning NGC and Switch options (NGC visible
  by default, Switch after show-all), a Wii entry, no-platform entry and a fully filtered answer.
- [ ] 1.4 Add the dialog chip/switch and “Show all” action, the filtered-empty notice, en/pt-BR
  labels and a Pikmin/NGC mock fixture. Verify keyboard/focus behavior is unchanged.
- [ ] 1.5 Add UI tests for default filtering, toggling off, empty-filtered state and an entry with
  no platform.
- [ ] 1.6 Run focused core/UI tests, `tsc`, the UI build, `cargo fmt`/`git diff --check` and
  strict OpenSpec validation; rebuild the desktop and manually confirm the Pikmin GameCube vs
  Switch case before marking the tasks complete.

## Out of Scope

- Server-side platform filters, confidence changes, automatic suggestion ranking and a persisted
  user preference. These remain follow-ups.
