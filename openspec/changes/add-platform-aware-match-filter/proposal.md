# Proposal

## Why

A provider record can declare the content platform it belongs to (for example the Dolphin
provider stores `platform = "gamecube"` or `"wii"`). The GameNode match dialog currently searches
by title only and mixes every platform: a GameCube Pikmin entry shows the GameCube game beside
Switch and other releases. The catalog search result already carries platform abbreviations
(`NGC`, `Wii`, `Switch`, ...), but nothing uses them.

Users can narrow the result themselves with a platform filter derived from the entry's record,
without changing the server search API or adding store-specific code to the host.

## What Changes

- **Derive a platform filter from the entry.** When a library entry declares a content platform,
  map it to the catalog's platform abbreviations (`gamecube` → `NGC`/`GC`/`GameCube`, `wii` →
  `Wii`) and mark the match dialog for that platform.
- **Filter the dialog options.** With the filter enabled (the default when the platform is known),
  candidates and search results whose platform does not intersect the declared platform are
  hidden. The search request itself is unchanged; this is a local narrowing of the answer.
- **Keep an explicit escape hatch.** The UI shows a “Platform: GameCube” toggle and “Show all”.
  Turning the filter off restores the complete result list. Entries without a declared platform
  behave exactly as today.
- **Explain an empty narrowed list.** When the search answered but the filter hides everything, the
  dialog says so and offers “Show all” instead of looking like a failed search.
- **Keep matching semantics.** Confidence, year detection, manual choices, “Not on GameNode”,
  current-match preview and the automatic suggestion engine are unchanged in this change.

## Capabilities

### Modified Capabilities

- `desktop-game-matching`: the match dialog may narrow candidates and results to the entry's
  declared content platform, with an explicit show-all escape hatch.

## Impact

- **`crates/core/src/library/match_dialog.rs`**: platform-filter state, mapping, view field and
  input action; option lists and default selection use the filtered set.
- **UI (`ui/src/features/dialogs/MatchDialog.tsx`)**: toggle/chip, empty-narrowed notice and
  show-all action; no new page.
- **i18n/mocks/tests**: en and pt-BR labels, a Dolphin/Pikmin fixture and focused tests.
- **No server, provider-plugin, store or API change.** Platform aliases are local data; the search
  endpoint remains text-only. Windows and Linux behave identically for any provider that supplies
  a platform field.

## Out of Scope

- Server-side platform search filters or changes to the search API.
- Changing confidence scoring or the automatic suggestion ranking (a follow-up may reuse the same
  mapping).
- Hiding the entry's current match or preventing manual cross-platform matches when the filter is
  off.
