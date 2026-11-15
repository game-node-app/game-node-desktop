# Spec Delta

## ADDED Requirements

### Requirement: Match suggestions in the detail pane
With a verified session, selecting one unmatched or ambiguous entry SHALL search GameNode for its detected title and show up to two suggestions scored by title similarity and release year, each with its confidence. Only suggestions at 40% or more SHALL be shown. Use SHALL match the entry to that game as a chosen match. Entries marked as not a game SHALL NOT get suggestions.

#### Scenario: Suggestions shown
- **WHEN** the user selects an unmatched DRM-free entry detected as "hollow_knight"
- **THEN** the pane lists up to two GameNode games with their covers, years and confidence, the best first

#### Scenario: Use a suggestion
- **WHEN** the user chooses Use on a suggestion
- **THEN** the entry is matched to that game as the user's choice, its cover is downloaded, and later lookups do not replace it

#### Scenario: No good suggestion
- **WHEN** no search result scores 40% or more
- **THEN** no suggestions section is shown and Match stays available

#### Scenario: Offline
- **WHEN** GameNode cannot be reached
- **THEN** no suggestion request is sent and the no-match notice still offers Match
