# Spec Delta

## ADDED Requirements

### Requirement: Achievement list summary and filter counts
The full achievement list SHALL show how many achievements are unlocked out of the total, as a count, a percentage and a progress bar. Each filter (all, unlocked, locked) SHALL show how many achievements it contains. The refresh time of the stored list SHALL be shown relative to now.

#### Scenario: Summary
- **WHEN** a game has 58 achievements and 31 are unlocked
- **THEN** the list shows 31 of 58 unlocked, 53%, and the filters show All 58, Unlocked 31 and Locked 27

#### Scenario: Refresh time
- **WHEN** the stored list was last refreshed from Steam five minutes ago
- **THEN** the window says it was updated from Steam 5 minutes ago

### Requirement: Global achievement rarity
For providers that publish it, the application SHALL store each achievement's global unlock percentage with the game's achievements and show it in the full list. Achievements unlocked by fewer than 10% of players SHALL be marked rare. A rarity that cannot be read SHALL NOT fail the achievement refresh; the list SHALL be shown without rarity.

#### Scenario: Steam rarity
- **WHEN** Steam reports that 4.2% of players unlocked an achievement
- **THEN** its row says Rare · 4.2% of players

#### Scenario: Rarity unavailable
- **WHEN** the global percentages request fails while the unlock state is read successfully
- **THEN** the achievements are stored and shown with no rarity, and the previous rarity values are kept if any

#### Scenario: Provider without rarity
- **WHEN** the game's provider does not publish global percentages
- **THEN** rows show no rarity and the rarity sorts are not offered

### Requirement: Sort the achievement list
The full list SHALL offer sorting by most recent (the default order: unlocked newest first, then locked), rarest first, most common first, and name. Achievements without rarity SHALL sort after those with rarity. The chosen sort SHALL apply within the active filter.

#### Scenario: Rarest first
- **WHEN** the user chooses Rarest on a list with rarity
- **THEN** achievements are ordered from the lowest to the highest global percentage, unlocked and locked together

#### Scenario: Hidden achievements stay hidden when sorted
- **WHEN** the list is sorted by name and includes a locked hidden achievement
- **THEN** it is placed by its generic label and its name and description stay hidden

### Requirement: Open the provider's achievements page
When the provider has a public achievements page for the game, the full list SHALL offer opening it in the default browser.

#### Scenario: Open in Steam
- **WHEN** the user chooses Open in Steam on a Steam game's achievement list
- **THEN** the default browser opens that game's Steam achievements page

#### Scenario: Browser fails
- **WHEN** no browser can be opened
- **THEN** a localized error is shown and the list stays open
