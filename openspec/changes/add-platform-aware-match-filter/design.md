# Design

## Context

`StoredEntry.fields` already carries provider-record fields, including the Dolphin plugin's
`platform` (`gamecube`/`wii`). The match dialog (`crates/core/src/library/match_dialog.rs`) builds
`MatchOption`s from server search answers, whose `platforms` are IGDB-style abbreviations (`NGC`,
`Wii`, `Switch`, ...). The search request only contains a query, so the filter must be local.

## Goals / Non-Goals

**Goals**

- Narrow candidates/results to the entry's declared platform by default when it is known.
- Keep a visible, remembered-in-dialog “Show all” control.
- Make an entirely filtered answer explain itself rather than looking empty or failed.
- Leave scoring, server search, provider plugins and the automatic suggestion engine unchanged.

**Non-Goals**

- Server-side filtering or new API parameters.
- Persisting the toggle across entries/dialogs.
- Inferring a platform from a title, ROM header or store id when the entry does not declare one.
- Changing confidence or automatic matching.

## Decisions

### D1. Platform source and mapping

Read `fields["platform"]` from the entry. Normalize it to lowercase and map to a small alias set:

| Declared | Accepted catalog abbreviations |
| --- | --- |
| `gamecube` | `NGC`, `GC`, `GCN`, `GameCube` (case-insensitive) |
| `wii` | `Wii` (case-insensitive) |

An unknown declared platform, an absent field or an empty alias set disables the filter. Aliases
are local constants; no provider-specific branch is added to the host.

### D2. Filter in the core, not only in React

The dialog state holds the derived platform and whether filtering is enabled. `candidate_options`
and `result_options` omit options that do not intersect the alias set when enabled. The default
selection is computed from the filtered options, so the preview and the visible list agree.
`MatchDialogView` exposes:

- `platformFilter`: optional `{ platform, label, aliases, enabled }`;
- `filteredOut`: count of options hidden by the active filter;
- the filtered `candidates`/`results` and the selected option.

Adding `SetMatchPlatformFilter(bool)` to `Input` lets the UI toggle the filter without a search
request. Turning it off re-runs the default selection so the dialog shows the best option from the
full answer.

### D3. UI behavior

The search form gains a compact chip/switch: “Platform: GameCube (NGC)”.
It is on when the platform is known. “Show all”/the switch turns it off. When the filtered list is
empty and the server returned games, the dialog shows “No GameCube results — Show all” and keeps
the search field available. The current-match preview remains selectable; if it is hidden by the
filter, the default selection uses the best visible option, and turning the filter off restores it.

### D4. Backward compatibility

Entries with no platform field retain today’s exact behavior: all options, no chip, no filtering.
When the server returns an option with no platform abbreviations, it is hidden while the filter is
on and returns when the filter is off. Windows, Linux, every provider and manual matching use the
same code; no plugin change is required.

### D5. Future automatic suggestions

The same alias mapping can later rank same-platform games higher in `suggest.rs`, but that changes
automatic behavior and is not part of this change. The match dialog filter is explicit and
reversible on purpose.

## Risks / Trade-offs

- **Catalog abbreviation variations** → include conservative aliases and treat unknown values as
  non-matching only while the filter is on; show-all restores them.
- **Hidden valid cross-platform matches** → default-on filter can hide a desired Switch release;
  the visible chip and show-all make the escape immediate.
- **Empty filtered view looks like a failed search** → explicit filtered-empty notice with the
  show-all action.
- **Entry fields missing/old cache** → no filter and today’s behavior.

## Migration Plan

1. Add the core state/mapping/view/input and tests.
2. Add the UI chip, i18n, mock and tests.
3. Run focused core/UI checks and rebuild; no data migration is required because the toggle is
   per-dialog and the underlying records are unchanged.
4. Rollback: remove the filter from the view; the search answer remains complete and no persisted
   state changes.
