## ADDED Requirements

### Requirement: Match search narrows to the declared platform

When a library entry declares a content platform, the match dialog SHALL derive a platform filter
from that record and SHALL show only candidates and search results whose catalog platform
abbreviations intersect it. Entries without a declared platform SHALL keep the current unfiltered
behavior. The search request SHALL remain text-only.

#### Scenario: GameCube entry with mixed search results

- **WHEN** a GameCube entry opens Match and the search returns an NGC game and a Switch game
- **THEN** only the NGC option is visible by default and the dialog shows the GameCube filter

#### Scenario: Entry has no declared platform

- **WHEN** the entry record has no platform field or an unknown platform
- **THEN** no filter is applied and every candidate/result is shown as today

### Requirement: Platform filtering is reversible and explains an empty narrowing

The platform filter SHALL be enabled by default when the platform is known and SHALL expose a
show-all action. When the filter hides every server result, the dialog SHALL distinguish that from
a failed search and keep the search field available. Turning the filter off SHALL restore the full
result list and re-apply the default selection.

#### Scenario: Filter hides every result

- **WHEN** the active platform filter matches none of the server's games
- **THEN** the dialog explains the narrowed-empty state, offers show-all, and keeps the search
  field available

#### Scenario: Show all restores the answer

- **WHEN** the user turns the filter off after it hid results
- **THEN** the complete candidate/result list returns and the default selection is recomputed
  without changing confidence, current-match or confirmation semantics
