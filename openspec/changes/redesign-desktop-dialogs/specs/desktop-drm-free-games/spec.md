# Spec Delta

## MODIFIED Requirements

### Requirement: Repair and remove local entries
Unavailable manual entries SHALL remain visible with Locate executable and Remove actions. Repair SHALL retain identity, matches, playtime and backup history. Removal SHALL only remove the registration; it SHALL NOT delete installed game files, live saves or backup archives. Remove SHALL offer two explicit choices, each describing its consequence: remove and suppress rediscovery until explicitly reconsidered (the default), or only remove so a later scan can find it again. Removing a watch SHALL stop discovery without removing approved games.

#### Scenario: Move a game
- **WHEN** an executable has moved and the user locates its new path
- **THEN** the same entry becomes playable with its existing history and matches

#### Scenario: Remove a game registration
- **WHEN** the user removes a DRM-free entry with the default choice and the watched folder rescans
- **THEN** the entry is absent and is not suggested again until the user resets its ignored state or explicitly adds it
- **AND** the installation, saves and backup archives remain on disk

#### Scenario: Only remove
- **WHEN** the user chooses Just remove for an entry in a watched folder and the folder rescans
- **THEN** the installation can be discovered again
- **AND** the installation, saves and backup archives remain on disk

## ADDED Requirements

### Requirement: Custom cover for a DRM-free game
Game settings SHALL let the user choose a local image as a DRM-free game's cover. The image SHALL be copied into the application's cover cache, so moving or deleting the original does not affect it, and SHALL take precedence over the GameNode cover. The user SHALL be able to return to the GameNode cover. Files that are not a readable PNG, JPEG or WebP image SHALL be rejected with a localized error.

#### Scenario: Change the cover
- **WHEN** the user chooses Change cover and picks a PNG for a DRM-free game, then saves
- **THEN** the library, detail pane and dialogs show that image for the game, also after the original file is deleted

#### Scenario: Matched game with a custom cover
- **WHEN** a DRM-free game with a custom cover is matched to a GameNode game
- **THEN** the custom cover is still shown

#### Scenario: Back to the GameNode cover
- **WHEN** the user chooses Use the GameNode cover and saves
- **THEN** the custom image is no longer shown and the GameNode cover, or the title placeholder, is shown

#### Scenario: Unreadable image
- **WHEN** the chosen file is not a readable image
- **THEN** an error is shown and the cover is unchanged

#### Scenario: Cancel keeps the old cover
- **WHEN** the user picks an image and then cancels Game settings
- **THEN** the previous cover is still shown

### Requirement: Discovery review suggests matches and flags likely non-games
The discovered-games review SHALL show, for each candidate, its title, folder, chosen executable and the most confident GameNode match suggestion at 40% or more, or that there is none. Candidates without positive game evidence whose executable or folder looks like an installer, redistributable or tool SHALL be marked probably not a game and SHALL start unchecked; other candidates SHALL start checked.

#### Scenario: Suggested match
- **WHEN** a candidate "Moss & Machine" is discovered while GameNode can be reached
- **THEN** its row shows the suggested GameNode game with its year, and adding it applies that suggestion as an automatic match

#### Scenario: Probably not a game
- **WHEN** a candidate's only executable is `vc_redist.x64.exe` in an `Instaladores` folder
- **THEN** its row says probably not a game and it is unchecked

#### Scenario: Offline review
- **WHEN** GameNode cannot be reached while the review is open
- **THEN** rows show no suggestion and the review can still add or ignore candidates

### Requirement: Ignoring discoveries can allow rediscovery
Ignore selected SHALL hide the selected candidates from the review. When the user leaves "Allow rediscovery in future scans" checked, ignored candidates SHALL be forgotten so a later scan can propose them again; when unchecked, they SHALL stay ignored until the user resets ignored items.

#### Scenario: Ignore with rediscovery
- **WHEN** the user ignores a candidate with rediscovery allowed and the folder is scanned again
- **THEN** the candidate appears in the review again

#### Scenario: Ignore permanently
- **WHEN** the user ignores a candidate with rediscovery not allowed and the folder is scanned again
- **THEN** the candidate does not appear until ignored items are reset

### Requirement: Tracking conflicts show both entries
When a DRM-free registration would track a game already tracked by a store entry, the application SHALL show both entries side by side, with title, cover, source and the store entry's playtime or the registration's folder, and SHALL offer using the existing entry or moving tracking to the DRM-free registration. Either choice SHALL count playtime once.

#### Scenario: Conflict with a Steam game
- **WHEN** the user registers a DRM-free copy of a game already tracked through Steam with 15 hours
- **THEN** the dialog shows the Steam entry with 15 h and the DRM-free entry with its folder, and the chosen entry alone tracks future sessions

#### Scenario: Cancel the conflict
- **WHEN** the user cancels the conflict dialog
- **THEN** tracking stays with the store entry and the DRM-free registration is not saved
