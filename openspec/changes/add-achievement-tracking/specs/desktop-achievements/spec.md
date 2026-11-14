# Spec Delta

## Purpose

Read a player's achievements from store providers, keep them on this PC per store account, refresh them without disturbing the app, and show them offline without any GameNode server dependency.

## ADDED Requirements

### Requirement: Providers declare an achievement backend
A provider manifest MAY declare `achievements = { plugin = "<id>" }`. The declared plugin SHALL be registered for that provider and granted the achievements capability, and manifest validation SHALL reject any other backend. A provider without the declaration SHALL offer no achievements.

#### Scenario: Steam declares achievements
- **WHEN** the bundled Steam manifest loads with its registered plugin
- **THEN** Steam entries are eligible for achievement reads

#### Scenario: Unregistered backend
- **WHEN** a manifest names an achievement plugin that is not registered for that provider
- **THEN** the manifest is rejected with a diagnostic and no achievement operation is sent

#### Scenario: Provider without achievements
- **WHEN** an entry belongs to a provider with no achievement backend, such as a DRM-free game
- **THEN** no achievement read is attempted and its detail pane shows no achievements section

### Requirement: Plugin achievement reads are scoped and bounded
The host SHALL request achievements only from a plugin granted the achievements capability, for a bounded batch of that provider's game ids. It SHALL reject results for ids it did not request, results over its limits, and results fenced out by a newer account or profile.

#### Scenario: Capability not granted
- **WHEN** a plugin without the achievements capability returns achievement data
- **THEN** the host rejects the result and stores nothing

#### Scenario: Unrequested game
- **WHEN** a result contains a game id outside the requested batch
- **THEN** the host rejects that result and stores nothing from it

#### Scenario: Result from a previous store account
- **WHEN** a result arrives after the store account changed or signed out
- **THEN** it is discarded

### Requirement: Steam achievements come from the web session
For an entry whose game id is a Steam app, the Steam plugin SHALL read achievement definitions and the connected account's unlock state using a short-lived token derived from the stored session. It SHALL NOT read Steam's local achievement files, and SHALL NOT persist the token.

#### Scenario: Connected account
- **WHEN** achievements are read for a game while Steam is connected
- **THEN** each achievement has a stable store id, name, description, hidden flag, icon references, unlocked state and any unlock time Steam reports

#### Scenario: Steam not connected
- **WHEN** Steam has no connected account
- **THEN** no achievement request is made and installed Steam games continue to work

#### Scenario: Local files present
- **WHEN** Steam's `appcache/stats` files exist on this PC
- **THEN** they are not opened

### Requirement: Local storage is the source of truth
Achievements SHALL be stored locally, keyed by provider entry and store account. Reading and displaying them SHALL require neither a GameNode session, nor a GameNode match, nor a reachable GameNode server.

#### Scenario: Unmatched game
- **WHEN** a Steam game has no GameNode match
- **THEN** its achievements are read, stored and shown normally

#### Scenario: GameNode unreachable
- **WHEN** GameNode cannot be reached and Steam can
- **THEN** achievement refresh and display are unaffected

#### Scenario: Different GameNode account
- **WHEN** another GameNode account signs in on this PC while the same Steam account stays connected
- **THEN** the same local achievements remain available

### Requirement: Achievement updates are atomic per game
A game's definitions and unlock state SHALL be replaced together from one complete read. A failed or partial read SHALL leave that game's previous data unchanged and SHALL record the failure category.

#### Scenario: Unlock state fails after definitions succeed
- **WHEN** Steam returns definitions but the unlock state request fails
- **THEN** the game's previously stored achievements are unchanged

#### Scenario: Definitions change
- **WHEN** a complete read no longer contains a previously stored achievement
- **THEN** that achievement is removed for the game

#### Scenario: Previously unlocked achievement stays unlocked
- **WHEN** a complete read reports an achievement as unlocked with the same unlock time
- **THEN** no duplicate or additional record is created for it

### Requirement: Games without achievements are distinguished
The application SHALL record a successful result of no achievements separately from never checked and from failed, and SHALL show each state accurately.

#### Scenario: Game has no achievements
- **WHEN** Steam reports that a game has no achievements
- **THEN** the game is recorded as checked with none, and its detail pane shows no progress bar

#### Scenario: Not checked yet
- **WHEN** a game has never been read
- **THEN** its detail pane shows a loading state and does not claim that it has no achievements

### Requirement: Refresh is automatic and never blocks the app
Achievement reads SHALL run off the UI thread, serialized per provider, and SHALL be requested when a stale game is selected, after a tracked game exits, by a paced library import, and by reconciliation of stale games at least every seven days.

#### Scenario: Selecting a stale game
- **WHEN** the user selects a game whose achievements are stale
- **THEN** cached data is shown immediately and that game is refreshed ahead of queued background work

#### Scenario: Game exits
- **WHEN** a tracked Steam game stops running
- **THEN** that game is refreshed once after a short settling interval

#### Scenario: Large library
- **WHEN** a library holds thousands of Steam games
- **THEN** background reads are paced, the app stays responsive, and the import resumes after a restart without re-reading fresh games

#### Scenario: Rate limited
- **WHEN** Steam reports rate limiting after bounded retries
- **THEN** background reads pause and resume later while cached data stays visible

### Requirement: Failures are categorized and preserve the cache
A failed read SHALL keep cached data visible and SHALL report a category without secrets. An expired session SHALL follow the existing store-account expiry behavior, and private game details SHALL be reported as a privacy failure.

#### Scenario: Steam unreachable
- **WHEN** Steam cannot be reached
- **THEN** cached achievements stay visible with their last successful update time

#### Scenario: Session expired
- **WHEN** Steam rejects the token
- **THEN** the account becomes expired, background reads stop, and cached achievements stay visible

#### Scenario: Private game details
- **WHEN** Steam does not disclose the account's achievements
- **THEN** the detail pane explains that the Steam privacy setting prevents reading them

#### Scenario: Secret-free diagnostics
- **WHEN** an achievement read fails
- **THEN** logs, errors and snapshots contain no cookie, token or full store response

### Requirement: Store account changes clear achievements
Explicit store sign-out, and replacement by a different store account, SHALL delete the previous account's stored achievements. Session expiry and unreachability SHALL NOT delete them.

#### Scenario: Sign-out
- **WHEN** the user signs out of Steam
- **THEN** that account's achievements are deleted and no longer shown

#### Scenario: Different Steam account connects
- **WHEN** a refresh identifies a different Steam account
- **THEN** the previous account's achievements are deleted before the new account's are shown

#### Scenario: Session expires
- **WHEN** the Steam session expires
- **THEN** stored achievements remain available

### Requirement: Detail pane shows an achievement summary
For an entry with an achievement backend, the detail pane SHALL show unlocked count, total, a progress indication not conveyed by color alone, the most recent unlock, and the last update time, read from local data. It SHALL offer opening the full list by keyboard and pointer.

#### Scenario: Offline
- **WHEN** the PC has no network and a game has cached achievements
- **THEN** the detail pane shows its summary from the cache

#### Scenario: Steam not connected and nothing cached
- **WHEN** Steam is not connected and a Steam game has no stored achievements
- **THEN** the detail pane explains that connecting Steam shows achievements

### Requirement: Full achievement list
The application SHALL show every achievement of a game in a movable, resizable window whose list stays responsive for large games. It SHALL show unlocked achievements first, newest first, followed by locked achievements, and SHALL allow showing all, unlocked or locked only.

#### Scenario: Open and close
- **WHEN** the user opens the full list and presses Escape
- **THEN** the window closes and focus returns to the control that opened it

#### Scenario: Unlock without a time
- **WHEN** an unlocked achievement has no unlock time
- **THEN** it is listed as unlocked after those with known times and shows no date

#### Scenario: Translated interface
- **WHEN** the application language is English or Brazilian Portuguese
- **THEN** interface text is translated and achievement text is requested in that language, with Steam's fallback shown when no translation exists

### Requirement: Hidden achievements do not spoil
A locked achievement that the store marks as hidden SHALL show a generic label and no store description until it is unlocked.

#### Scenario: Locked hidden achievement
- **WHEN** the list includes a locked hidden achievement
- **THEN** its name and description are replaced by generic text

#### Scenario: Unlocked hidden achievement
- **WHEN** a hidden achievement is unlocked
- **THEN** its real name and description are shown

### Requirement: Achievement icons are cached locally
Icons SHALL be downloaded only when a game's achievements are viewed, only over HTTPS from trusted store image hosts, and served from the local cache. A missing icon SHALL show a placeholder and SHALL NOT affect the list.

#### Scenario: Cached icon offline
- **WHEN** a previously viewed game's list is opened offline
- **THEN** its icons are shown from the cache

#### Scenario: Untrusted icon host
- **WHEN** an icon reference points outside the trusted hosts or is not HTTPS
- **THEN** it is not fetched and a placeholder is shown

### Requirement: Achievements are not reported to GameNode
The application SHALL NOT send achievement definitions, unlock state or derived counts to GameNode, regardless of the Sync with GameNode setting.

#### Scenario: Sync enabled
- **WHEN** Sync with GameNode is on and achievements are refreshed
- **THEN** no request containing achievement data is made to GameNode
