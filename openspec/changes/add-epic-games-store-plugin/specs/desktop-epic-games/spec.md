# Spec Delta

## Purpose

Bring Epic Games Store games into the GameNode library on Windows. The official Epic plugin covers installed discovery, launcher actions, an opt-in account with an owned catalog, and Epic playtime totals, using only the scoped host services.

## ADDED Requirements

### Requirement: Epic is a Windows plugin provider
The application SHALL load an `epic` provider on Windows only. It SHALL be backed solely by the official Epic plugin, through its trusted descriptor. On other platforms the provider SHALL be absent, and no Epic control SHALL appear. No Epic behavior SHALL run in the host process.

#### Scenario: Windows with the plugin packaged
- **WHEN** the desktop starts on Windows with the Epic executable next to the host
- **THEN** an Epic provider is listed with its installed games, account controls and plugin inventory entry

#### Scenario: Linux
- **WHEN** the desktop runs on Linux
- **THEN** no Epic provider, account card or inventory entry is shown, and other providers are unaffected

#### Scenario: Plugin missing
- **WHEN** the Epic executable is absent or fails to start on Windows
- **THEN** Epic is reported unavailable with an actionable status, its cached entries remain visible and all other providers keep working

### Requirement: Epic installed discovery
Discovery SHALL read the Epic Games Launcher's installed-item manifests from the launcher data directory. It SHALL report each fully installed base game with its application name as the provider id, its display name and its install folder. A missing launcher SHALL be a successful empty result.

#### Scenario: Installed games
- **WHEN** the launcher data directory holds manifests for two complete game installs
- **THEN** both games are reported with stable `epic:<appName>` keys and their install folders

#### Scenario: Launcher not installed
- **WHEN** no Epic launcher data directory exists
- **THEN** discovery succeeds with no games and previous Epic installed facts are cleared

#### Scenario: Launcher data unreadable
- **WHEN** the manifest directory exists but cannot be read
- **THEN** discovery reports a failure, and previous installed facts and matches remain

### Requirement: Epic discovery excludes non-games
Discovery SHALL skip DLC and add-ons whose main game differs from the item itself. It SHALL also skip incomplete or in-progress installs, Unreal Engine and other non-game launcher items, and manifests that are malformed or lack an application name or install folder. A skipped manifest SHALL NOT prevent other games from being reported.

#### Scenario: DLC next to its base game
- **WHEN** a base game and one of its DLC both have manifests
- **THEN** only the base game is reported

#### Scenario: Malformed manifest
- **WHEN** one manifest is not valid JSON
- **THEN** it is skipped with a secret-free log and the other games are still reported

### Requirement: Epic launcher actions
The plugin SHALL declare Play and Install URI templates using the `com.epicgames.launcher` scheme, filled from the provider id. The host SHALL validate and fill them like any other declared action. Play SHALL be offered for installed entries and Install for owned entries that are not installed.

#### Scenario: Play an installed game
- **WHEN** the user plays an installed Epic game
- **THEN** the OS handler receives the launcher URI for that application name and nothing else is executed

#### Scenario: Install an owned game
- **WHEN** the user chooses Install on an owned Epic game that is not installed
- **THEN** the launcher install URI for that application name is handed to the OS

#### Scenario: Launcher missing
- **WHEN** no handler is registered for the launcher scheme
- **THEN** the action reports that the Epic Games Launcher is required and the library is unchanged

### Requirement: Epic account sign-in
Epic sign-in SHALL be opt-in. It SHALL use the isolated sign-in window on Epic's own HTTPS login page. Within that window the Epic plugin SHALL obtain a one-time authorization code through the approved page probe and exchange it for tokens. Social-login navigation outside Epic hosts SHALL open in the system browser.

#### Scenario: Successful sign-in
- **WHEN** the user completes Epic's login, including any two-factor step
- **THEN** the window closes, the account becomes connected with its Epic display name, and an owned-catalog sync starts

#### Scenario: User cancels or closes
- **WHEN** the user closes the window, cancels, or the attempt times out
- **THEN** no credential is saved and the previous Epic account state is unchanged

#### Scenario: Authorization code rejected
- **WHEN** Epic rejects the code exchange
- **THEN** sign-in fails with a secret-free error and no credential is saved

### Requirement: Epic credential handling
The Epic plugin SHALL keep only its refresh token in its own provider secret slot. It SHALL keep the access token in memory only. It SHALL replace the stored refresh token whenever Epic rotates it. The authorization code, tokens and account identifiers SHALL NOT appear in logs, errors, snapshots or GUI state.

#### Scenario: Restart with a valid refresh token
- **WHEN** the desktop starts and Epic accepts the stored refresh token
- **THEN** the account becomes connected without another sign-in and the rotated refresh token replaces the stored one

#### Scenario: Refresh token revoked or expired
- **WHEN** Epic rejects the stored refresh token as invalid
- **THEN** the account becomes expired, the dead credential is deleted, and cached library facts remain visible

#### Scenario: Epic unreachable
- **WHEN** the token endpoint cannot be reached or returns a server error
- **THEN** the credential is kept, the account becomes unreachable, and cached facts remain visible

### Requirement: Epic owned catalog
A connected Epic account SHALL report its complete set of owned base games as the owned category. Each game SHALL use the same application-name id as discovery, so that installed and owned facts merge into one entry. DLC, engine items and items without a launchable application name SHALL be excluded. Epic SHALL NOT report a family category.

#### Scenario: Owned games sync
- **WHEN** a connected account owns three base games, one of them installed
- **THEN** the library shows three Epic entries, and the installed one carries both installed and owned facts

#### Scenario: Paged library fails midway
- **WHEN** a later page of the owned library request fails
- **THEN** the owned category is reported as failed and the previous owned facts remain

#### Scenario: Empty library
- **WHEN** the account successfully reports no owned games
- **THEN** previous Epic owned facts are cleared

### Requirement: Epic playtime totals
The Epic plugin SHALL report total playtime per application name through the counter service when the account is connected. It SHALL report the last-played time as unknown when Epic provides none. Without a connected account it SHALL report no observations, and this SHALL NOT be treated as zero playtime.

#### Scenario: Store source selected
- **WHEN** the Store playtime source is selected and a matched Epic game exits
- **THEN** Epic's total for that game is read and submitted as the `epic` total, following the existing once-per-change rules

#### Scenario: No last-played date
- **WHEN** Epic returns a total without a last-played time
- **THEN** the entry's last played stays unknown rather than showing 1970

#### Scenario: Signed out
- **WHEN** no Epic account is connected
- **THEN** no Epic totals are read or submitted, and the previously accepted totals are not overwritten

### Requirement: Epic games match through GameNode
Epic entries SHALL be matched automatically through GameNode's external lookup, using GameNode's Epic namespace category and the entry's Epic catalog namespace as the external id. Installed and owned Epic entries SHALL both carry that namespace. Entries with no answer SHALL stay unmatched for the user to resolve, and a user's manual match SHALL always win.

#### Scenario: External id known to GameNode
- **WHEN** GameNode returns a game for an Epic entry's catalog namespace
- **THEN** the entry is matched automatically

#### Scenario: Owned but not installed
- **WHEN** an owned Epic game that is not installed is synced
- **THEN** its entry carries the catalog namespace and is looked up like an installed one

#### Scenario: Entry without namespace
- **WHEN** an Epic entry has no catalog namespace
- **THEN** no lookup is sent for it and it stays unmatched

#### Scenario: External id unknown
- **WHEN** the external lookup returns no game for an Epic entry
- **THEN** the entry stays unmatched and can be matched manually like any other provider entry
