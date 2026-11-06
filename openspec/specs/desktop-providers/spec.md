# desktop-providers Specification

## Purpose

Describe each game source (a store, a launcher or manual adds) as a provider manifest, turn what is on disk into plain game records, and keep launch and install limited to what each manifest declares.

## Requirements

### Requirement: Bundled provider manifests
The application SHALL embed the TOML provider manifests from the repository's `providers/` directory at build time. It SHALL load a manifest only on the platforms it lists. It SHALL NOT read manifests from user-writable locations.

#### Scenario: Steam manifest on a supported platform
- **WHEN** the application starts on Windows or Linux
- **THEN** the bundled Steam provider is available without any file outside the application binary

#### Scenario: Manifest for another platform
- **WHEN** a bundled manifest does not list the current platform
- **THEN** that provider is not loaded and not shown as a filter

### Requirement: Manifest validation isolates failures
The application SHALL validate every manifest when it loads. Validation SHALL reject unknown fields, missing required fields, an unknown native scanner, an unknown playtime counter, a duplicate provider id, malformed placeholders, and invalid action declarations. Executable launch SHALL be restricted to the bundled manual provider; other providers SHALL retain declared URI actions. An invalid manifest SHALL be skipped and logged, and the other manifests SHALL still load.

#### Scenario: One manifest is invalid
- **WHEN** one bundled manifest fails validation
- **THEN** it is skipped with a diagnostic naming the provider and the problem
- **AND** every valid provider still scans and launches

#### Scenario: Unknown native scanner
- **WHEN** a manifest names a native scanner the application does not have
- **THEN** that manifest is rejected and no scan runs for it

#### Scenario: Unknown playtime counter
- **WHEN** a manifest names a playtime counter the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the counter

#### Scenario: Undeclared executable action
- **WHEN** a non-manual provider declares an executable action
- **THEN** it is rejected and no executable can be launched through that declaration

### Requirement: Scanners return plain records
Each scan SHALL produce records with a provider-local `id`, an optional `name`, an `install_dir`, and further named text fields. Native and generic scanners SHALL produce the same record shape, so the runtime treats them alike. A record without a usable `id` or `install_dir` SHALL be dropped and logged.

#### Scenario: Native scanner output
- **WHEN** the Steam scanner reads an installed game
- **THEN** the runtime receives a record with that game's app id, name and install folder

#### Scenario: Incomplete record
- **WHEN** a scanner encounters a game entry with no id or no install folder
- **THEN** that entry is left out of the library and the rest of the scan continues

### Requirement: Placeholder filling
The runtime SHALL fill a manifest's `{placeholder}` values from the record's fields. A template that refers to a field the record lacks SHALL NOT produce a launch or install target. Filled values SHALL be percent-encoded and SHALL NOT change the URI's scheme, authority or path structure.

#### Scenario: Fill a launch URI
- **WHEN** a Steam record with id `620` is launched with template `steam://rungameid/{id}`
- **THEN** the target is `steam://rungameid/620`

#### Scenario: Value with reserved characters
- **WHEN** a field value contains `/`, `?`, `#`, `%`, whitespace or control characters
- **THEN** the value is encoded or rejected, and the filled URI cannot gain extra path segments, queries or schemes

#### Scenario: Missing field
- **WHEN** a template refers to a field that the record does not have
- **THEN** the action is unavailable for that game and nothing is opened

### Requirement: Launch and install are restricted to declared URIs
URI Play and Install SHALL open only a filled URI whose scheme is the scheme of the manifest's own template, passed as one argument without a shell. The bundled manual provider SHALL additionally permit Play through a user-approved executable inside its registered game folder, with a stored argument list and working directory. Manual entries SHALL have no Install action. Executable targets SHALL be revalidated before each launch.

#### Scenario: Play a Steam game
- **WHEN** the user plays a detected Steam game
- **THEN** the OS opens `steam://rungameid/<id>` with the registered Steam handler

#### Scenario: Install through the store client
- **WHEN** the user chooses Install for a Steam game
- **THEN** the OS opens `steam://nav/games/details/<id>` and the application writes nothing to game folders

#### Scenario: Handler is missing
- **WHEN** the OS has no handler for the scheme or opening fails
- **THEN** the application shows a localized error naming the store and the library is unchanged

#### Scenario: Direct executable launch
- **WHEN** the user plays an available DRM-free entry
- **THEN** its approved executable is started with its stored argument list and working directory without invoking a shell

#### Scenario: Escaping the registered folder
- **WHEN** an executable resolves through a replaced junction outside its registered game folder
- **THEN** launch is refused and the entry offers repair

### Requirement: Steam roots
The Steam provider SHALL find Steam at the Windows registry value `HKCU\Software\Valve\Steam\SteamPath`, and on Linux at the native locations `~/.steam/steam` and `~/.local/share/Steam` and at Flatpak Steam's data folder. Locations that resolve to the same folder SHALL be scanned once. A missing location SHALL NOT be an error.

#### Scenario: Native and Flatpak Steam on one Linux PC
- **WHEN** both native and Flatpak Steam are installed with different games
- **THEN** games from both appear in the library

#### Scenario: Symlinked native root
- **WHEN** `~/.steam/steam` is a link to `~/.local/share/Steam`
- **THEN** each game appears once

#### Scenario: Steam not installed
- **WHEN** no Steam location exists
- **THEN** the scan finishes with no Steam games and no error message

### Requirement: Steam library scanning
The Steam scanner SHALL read every library folder that `libraryfolders.vdf` lists and each `appmanifest_*.acf` in it. A game SHALL be included when its app manifest has a numeric app id and its install folder exists. Steam runtimes, Proton builds and redistributables that the manifest lists as excluded SHALL NOT appear.

#### Scenario: Games across several library folders
- **WHEN** Steam has games in its main folder and on a second drive
- **THEN** games from every listed folder appear with their install folders

#### Scenario: Unreadable or malformed file
- **WHEN** a library folder is offline or one app manifest is malformed
- **THEN** that folder or game is skipped and logged, and the other games still appear

#### Scenario: Steam tools are hidden
- **WHEN** Steam Linux Runtime, Proton or Steamworks Common Redistributables are installed
- **THEN** they are not shown as games

### Requirement: Manifest playtime counter
A manifest MAY declare `playtime = { counter = "<name>" }`, naming a store counter built into the application. With Store selected, a provider with a counter SHALL use that counter. With Internal selected, or without a supported counter, it SHALL use measured sessions. A counter that cannot read a game's total SHALL leave that game's total unchanged, not set it to zero.

#### Scenario: Provider without a counter
- **WHEN** a manifest has no `playtime` field
- **THEN** its games' play is recorded as measured sessions

#### Scenario: Counter cannot read a game
- **WHEN** the counter's file is missing, unreadable or has no entry for a game
- **THEN** no total is submitted for that game and the failure is logged without file contents

### Requirement: Steam playtime counter
When the Steam manifest declares the `steam` counter, the application SHALL read each Steam account's per-game total and last-played time from that account's `localconfig.vdf` under every scanned Steam root. A game's total SHALL come from the Steam account that played it most recently. Fields SHALL follow `docs/steam-playtime-counter.md`: Playtime minutes converted to seconds and LastPlayed Unix seconds.

#### Scenario: One Steam account
- **WHEN** one Steam account on the PC has 754 minutes recorded for app `620`
- **THEN** the total for `steam:620` is 45240 seconds, with that account's last-played time

#### Scenario: Two Steam accounts played the same game
- **WHEN** two Steam accounts on the PC have totals for app `620`
- **THEN** the total from the account with the later last-played time is used

#### Scenario: Malformed config
- **WHEN** one account's `localconfig.vdf` is malformed
- **THEN** that account is skipped and logged, and the other accounts' totals are still read

### Requirement: Built-in DRM-free provider
Windows SHALL include a bundled manual provider whose scan reads approved local registrations, whose display name is DRM-free and whose playtime source is `manual`. Its ids SHALL be installation identities rather than catalog or manifest ids. Unapproved folder discoveries SHALL never be scanner records. It SHALL not declare a store counter.

#### Scenario: Scan manual registrations
- **WHEN** a scan runs after an executable was manually added
- **THEN** its `manual:<installation id>` entry appears alongside Steam entries without requiring another picker interaction

#### Scenario: GameNode identity changes
- **WHEN** the GameNode match or Ludusavi profile changes
- **THEN** the provider id, installation id and `manual` playtime source stay unchanged
