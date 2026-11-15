# Spec Delta

## MODIFIED Requirements

### Requirement: Favorites in the library view
The library SHALL show a favorite indicator in grid and list, offer Add to favorites or Remove from favorites in the context menu, the detail pane, the list's favorite column and the F shortcut, and provide favorites through the Favorites scope and the Favorites only filter. With several entries selected, it SHALL offer Favorite all, or Unfavorite all when every selected matched entry is a favorite, applied to the selected matched entries only. The actions and the filter SHALL be keyboard accessible and available in English and Brazilian Portuguese.

#### Scenario: Indicator
- **WHEN** a favorited game is visible in grid or list
- **THEN** its tile or row shows a star indicator that is announced to assistive technology

#### Scenario: Context menu
- **WHEN** the user opens the context menu on a matched entry
- **THEN** the menu offers Add to favorites or Remove from favorites according to its current state

#### Scenario: Favorites filter
- **WHEN** the user selects the Favorites scope or turns on Favorites only
- **THEN** only favorited entries remain, and the active-filter chip can remove the filter

#### Scenario: Multi-selection
- **WHEN** three entries are selected, two matched and not favorited, and the user chooses Favorite in the batch bar
- **THEN** the two matched entries become favorites, the unmatched one is unchanged, and each change follows the same local-first and sync rules as a single favorite

#### Scenario: Unfavorite all
- **WHEN** every selected matched entry is already a favorite
- **THEN** the batch action reads Unfavorite and removes them all

#### Scenario: Keyboard
- **WHEN** the user reaches the favorite action or the Favorites filter with the keyboard, or presses F with a game selected
- **THEN** it can be activated with Enter or Space, F toggles the selected game's favorite, and its state is exposed
