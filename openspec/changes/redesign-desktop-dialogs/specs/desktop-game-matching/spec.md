# Spec Delta

## MODIFIED Requirements

### Requirement: Search for unmatched games
An entry without a match SHALL offer a match dialog that searches GameNode's existing game search, prefilled with the detected title. Each option SHALL show the title, release year, platforms when known, cover and a confidence percentage scored like match suggestions. Selecting an option SHALL preview it with the reasons for its confidence. The entry SHALL change only when the user confirms Match. The user SHALL be able to edit the query.

#### Scenario: Match by hand
- **WHEN** the user opens Match on an unmatched game, selects a search result and confirms Match
- **THEN** the entry is matched to that game and its cover is downloaded

#### Scenario: Selecting does not match
- **WHEN** the user selects an option and then cancels or closes the dialog
- **THEN** the entry keeps its current match

#### Scenario: Candidates come first and the best is preselected
- **WHEN** the server mapped the store id to several games and the dialog opens
- **THEN** those games are listed first, ordered by confidence, and the current match, or else the most confident option, is selected and previewed

#### Scenario: Confidence reasons
- **WHEN** the user selects an option whose title is 96% similar, whose release year equals a year in the detected name or path, and which the server offered for the store id
- **THEN** the preview lists a similar title with its percentage, the same release year, and a recognized store id as positive reasons

#### Scenario: Year differs
- **WHEN** a year was detected and the selected option was released in another year
- **THEN** the preview lists the different release year as a warning reason

#### Scenario: Search fails
- **WHEN** the search request fails or times out
- **THEN** the dialog shows a localized error and the entry keeps its current state
