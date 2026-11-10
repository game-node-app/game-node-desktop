# desktop-providers Specification

## Purpose

Describe each game source (a store, a launcher or manual adds) as a provider manifest, turn what is on disk into plain game records, and keep launch and install limited to what each manifest declares.

## Requirements

### Requirement: Bundled provider manifests
The application SHALL embed trusted bundled TOML provider manifests at build time and load them only on declared platforms. Plugin-backed manifests SHALL identify the packaged plugin and its supported operations. The application SHALL NOT load manifests from user-writable locations.

#### Scenario: Steam manifest on a supported platform
- **WHEN** the packaged application starts on Windows or Linux
- **THEN** the bundled Steam provider and its compatible external plugin are available without manual plugin installation

#### Scenario: Manifest for another platform
- **WHEN** a bundled manifest does not list the current platform
- **THEN** that provider is not loaded and not shown as a filter

#### Scenario: Declared plugin is missing
- **WHEN** the Steam plugin executable is absent or incompatible
- **THEN** the desktop reports its unavailability, preserves cached data and does not run an in-process Steam substitute

### Requirement: Manifest validation isolates failures
The application SHALL validate every manifest when it loads. Validation SHALL reject unknown fields, missing required fields, an unknown native scanner, an unknown catalog backend, an unknown playtime counter, a duplicate provider id, malformed placeholders, and invalid action declarations. Registered plugin operations SHALL be valid scanner, catalog and counter backends only within their approved descriptor. Executable launch SHALL be restricted to the bundled manual provider; other providers SHALL retain declared URI actions. An invalid manifest SHALL be skipped and logged, and the other manifests SHALL still load.

#### Scenario: One manifest is invalid
- **WHEN** one bundled manifest fails validation
- **THEN** it is skipped with a diagnostic naming the provider and the problem
- **AND** every valid provider still scans and launches

#### Scenario: Unknown native scanner
- **WHEN** a manifest names a native scanner the application does not have
- **THEN** that manifest is rejected and no scan runs for it

#### Scenario: Unknown catalog backend
- **WHEN** a manifest names a catalog backend that is neither available natively nor authorized through its registered plugin
- **THEN** that manifest is rejected with a diagnostic naming the backend and other providers remain usable

#### Scenario: Unknown playtime counter
- **WHEN** a manifest names a playtime counter the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the counter

#### Scenario: Undeclared executable action
- **WHEN** a non-manual provider declares an executable action
- **THEN** it is rejected and no executable can be launched through that declaration

#### Scenario: Valid plugin backend
- **WHEN** a manifest selects a scanner, catalog or counter capability from its registered plugin descriptor
- **THEN** the host accepts that operation without requiring a Steam-specific native backend

#### Scenario: Plugin capability exceeds its descriptor
- **WHEN** the manifest requests an operation or permission not approved for that plugin
- **THEN** the host rejects it and other valid providers remain usable

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
A manifest SHALL be allowed to declare a registered native or plugin store counter. With Store selected, a provider with a counter SHALL use that counter. With Internal selected, or without a supported counter, it SHALL use measured sessions. A counter that cannot read a game's total SHALL leave that game's total unchanged, not set it to zero.

#### Scenario: Provider without a counter
- **WHEN** a manifest has no `playtime` field
- **THEN** its games' play is recorded as measured sessions

#### Scenario: Counter cannot read a game
- **WHEN** the counter's file is missing, unreadable or has no entry for a game
- **THEN** no total is submitted for that game and the failure is logged without file contents

#### Scenario: Plugin counter fails
- **WHEN** a plugin counter cannot complete an authorized request
- **THEN** prior totals remain and no fabricated zero or duplicate internal session is created

### Requirement: Steam playtime counter
The Steam plugin SHALL read each Steam account's per-game total and last-played time from that account's `localconfig.vdf` under every discovered Steam root when its counter is requested. A game's total SHALL come from the Steam account that played it most recently. Fields SHALL follow `docs/steam-playtime-counter.md`: Playtime minutes converted to seconds and LastPlayed Unix seconds. Core SHALL retain attribution and submission policy.

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

### Requirement: Steam catalog capability
A manifest SHALL be allowed to declare a registered native or plugin catalog backend. A provider without a catalog SHALL have no account connection or owned-library import. Catalog records SHALL carry a provider-local id and a non-empty name and SHALL support a family-shared category; the manifest's `exclude_ids` SHALL apply to them. Steam SHALL use its plugin catalog on Linux and Windows.

#### Scenario: Steam manifest declares a catalog
- **WHEN** the bundled Steam manifest declares the Steam plugin catalog backend
- **THEN** Steam can list owned and family-shared games in addition to installed games

#### Scenario: Empty catalog record
- **WHEN** a catalog response contains an entry without a usable id or without a name
- **THEN** that entry is dropped and logged and the rest of the catalog is kept

#### Scenario: Excluded app
- **WHEN** an owned or family-shared app id appears in the manifest's `exclude_ids`
- **THEN** it does not become a library entry

### Requirement: Steam owned catalog
With a valid Steam session, the Steam catalog SHALL read every game the account owns, including uninstalled, unplayed, and free games, using the account's short-lived web API token. It SHALL keep numeric app ids with non-empty names, remove duplicates, and treat an empty library as a successful empty catalog. Transient Steam responses SHALL be retried a bounded number of times.

#### Scenario: Full owned library
- **WHEN** the account owns 320 games and only 18 are installed
- **THEN** the catalog yields all 320 app ids and names, and the 18 installed entries keep their install folders

#### Scenario: Empty library
- **WHEN** Steam successfully reports that the account owns no games
- **THEN** the owned catalog is empty and no error is shown

#### Scenario: Transient failure
- **WHEN** Steam answers with a rate limit or a server error
- **THEN** the request is retried a bounded number of times and then reported as a temporary failure

#### Scenario: Authentication rejected
- **WHEN** Steam rejects the web API token
- **THEN** the catalog reports an authentication rejection and the account becomes expired

### Requirement: Steam Family shared catalog
When the account belongs to a Steam Family, the catalog SHALL read the games shared with it and treat them as visible Steam entries distinct from owned games. An account in no family SHALL produce no family-shared games without an error. A family-shared game that the account also owns SHALL be treated as owned only.

#### Scenario: Family games
- **WHEN** the account belongs to a family that shares games it does not own
- **THEN** those games become visible Steam entries marked as family-shared

#### Scenario: No family group
- **WHEN** the account belongs to no Steam Family
- **THEN** the family catalog is empty and the owned catalog still succeeds

#### Scenario: Owned and shared
- **WHEN** a family-shared app is also in the account's owned catalog
- **THEN** the entry is marked owned and not family-shared

#### Scenario: Leave a family
- **WHEN** a successful family sync no longer lists a previously shared game
- **THEN** that entry loses its family-shared fact and disappears unless it is installed or owned

### Requirement: Catalog failures are isolated
A failed owned catalog fetch SHALL leave the previous owned facts unchanged. A failed family fetch SHALL leave the previous family facts unchanged and SHALL NOT fail the owned catalog or the installed scan. A completed sync SHALL replace only the fact set it covers.

#### Scenario: Owned catalog unreachable
- **WHEN** the owned catalog request fails
- **THEN** previously owned games remain visible and the provider records a catalog failure

#### Scenario: Family catalog fails
- **WHEN** the family catalog request fails while the owned catalog succeeds
- **THEN** owned games update, previously shared games remain, and Stores shows a family warning

#### Scenario: Installed scan fails
- **WHEN** the installed scan fails while the catalog succeeds
- **THEN** installed facts stay as they were and owned and family facts update

### Requirement: Independent installed, owned and family facts
The library SHALL represent installed, owned, and family-shared as independent facts for the same provider entry. An entry SHALL be visible when at least one fact is true. A completed installed scan, owned sync, or family sync SHALL update only its own fact set.

#### Scenario: Catalog-only game
- **WHEN** a game is owned but not installed
- **THEN** its entry is visible with an empty install folder

#### Scenario: Uninstalled but owned
- **WHEN** an owned game is uninstalled and a later installed scan no longer finds it
- **THEN** the entry stays visible as owned and is no longer installed

#### Scenario: Installed but not owned
- **WHEN** an installed Steam game is not in the owned or family catalogs
- **THEN** the entry stays visible as installed

#### Scenario: Failed installed scan
- **WHEN** the installed scan fails
- **THEN** installed facts are unchanged, including entries that were installed before the failure

### Requirement: Catalog entries keep identity and matches
A catalog entry SHALL use the same `<provider>:<store id>` key as an installed entry for the same app. It SHALL preserve the entry's first-seen time, chosen match, automatic match, cover, and playtime history when facts change or the entry is temporarily invisible. Clearing ownership SHALL NOT delete entries.

#### Scenario: Install a catalog game
- **WHEN** a catalog-only game is installed and the next scan finds it
- **THEN** the same entry becomes installed and keeps its match and cover

#### Scenario: Catalog game disappears
- **WHEN** an owned game is removed from the catalog
- **THEN** the entry becomes invisible but its match and cover are kept for a later re-add

#### Scenario: Sign out
- **WHEN** the Steam account signs out
- **THEN** owned and family-shared facts are cleared and entries, matches, and covers are kept

### Requirement: Catalog sync follows scans and account changes
A catalog sync SHALL be requested for every scan of a provider with a catalog and when its account becomes connected. An explicit rescan or Stores refresh SHALL request it too. Only one catalog sync per provider SHALL run at a time, and requests made while one runs SHALL coalesce.

#### Scenario: Startup
- **WHEN** the application starts with a connected account and cached catalog
- **THEN** the cached games are visible before the fresh catalog request finishes

#### Scenario: Manual rescan
- **WHEN** the user chooses Rescan
- **THEN** installed games and the owned and family catalogs are refreshed

#### Scenario: Background scan
- **WHEN** a periodic background scan runs shortly after a successful catalog sync
- **THEN** the catalog request may be skipped while installed games are still scanned

#### Scenario: Concurrent request
- **WHEN** another catalog sync is requested while one is running
- **THEN** no second request starts and the running result is used

### Requirement: Steam integration is provided through the plugin contract
On Linux and Windows, all Steam-specific discovery, authentication, session, account, HTTP, catalog and counter behavior SHALL be delegated to the Steam plugin. Provider-owned action declarations SHALL supply Play and Install. Core/Engine SHALL retain generic library policy and GameNode operations through the public plugin boundary.

#### Scenario: Native and Flatpak or Windows registry discovery
- **WHEN** Steam discovery is requested on a supported platform
- **THEN** the plugin finds the existing native, Flatpak or registry-defined roots and returns the same installed identities and paths under the existing Steam scan requirements

#### Scenario: Owned and family catalog
- **WHEN** the connected Steam account refreshes its library
- **THEN** the plugin supplies all owned and family games with the existing filtering, retry and separate-error behavior

#### Scenario: Store actions
- **WHEN** Play or Install is requested for installed or catalog-only Steam entries
- **THEN** the host validates the plugin-owned declaration and opens the same Steam URI without writing game files

#### Scenario: Plugin process is temporarily unavailable
- **WHEN** cached Steam entries and previously validated action declarations exist during a plugin failure
- **THEN** existing cached Play/Install actions remain usable through generic host execution where their inputs remain valid

### Requirement: Steam migration preserves mainline behavior and data
The plugin migration SHALL preserve the working mainline Steam account, catalog, library and applicable playtime behavior on Linux and Windows. Existing profiles SHALL retain identities, credentials, matches, favorites, covers and history. Generic GameNode and other-provider behavior SHALL remain compatible with the integrated mainline baseline.

#### Scenario: Upgrade and restart
- **WHEN** an existing signed-in Steam profile is upgraded and restarted with a still-valid credential
- **THEN** the account reconnects without another login and its cached entries, metadata and preferences remain available

#### Scenario: Cancelled or unsuccessful login
- **WHEN** a login is cancelled, closed, times out or fails
- **THEN** the existing account's prior state and the current mainline recovery behavior are preserved

#### Scenario: Session expires or Steam is unreachable
- **WHEN** validation rejects the session or Steam temporarily cannot be reached
- **THEN** the existing expired/unreachable states, credential handling, cached-library banners and actions are preserved

#### Scenario: Sign-out or account replacement
- **WHEN** a sign-out succeeds or a verified different Steam identity replaces the account
- **THEN** old owned/family facts are cleared under existing rules while installed games, saved matches, covers and history remain

#### Scenario: Library updates during browsing
- **WHEN** plugin results change installed, owned or family facts
- **THEN** the library preserves filters, search, sort, surviving selection, ownership indicators, deduplication and current snapshot performance behavior

#### Scenario: Windows Store and Internal tracking
- **WHEN** Windows observes Steam gameplay under either playtime preference
- **THEN** Core preserves process tracking, account ownership, counter triggers, pending totals, deduplication and local measured-session behavior

#### Scenario: Linux tracking availability
- **WHEN** the migrated application runs on Linux
- **THEN** it preserves the mainline platform availability of process tracking without inventing Linux gameplay sessions

### Requirement: Steam plugin integration preserves PC ports behavior
The application SHALL retain mainline PC ports catalog, overlay, guided setup, installation, progress, cancellation, recovery and DRM-free registration behavior on Windows. Steam migration SHALL preserve port identities, provenance, launch arguments, environment and gameplay tracking. PC ports SHALL remain independent of Steam account and plugin availability, with its existing platform scope.

#### Scenario: Existing port is launched after upgrade
- **WHEN** a schema-8 profile containing an installed port opens in the integrated application
- **THEN** its registration, provenance, executable roles, arguments and environment remain available for normal DRM-free launch and tracking

#### Scenario: Steam is signed out or unavailable
- **WHEN** Steam has no connected account or its plugin cannot run
- **THEN** PC ports browsing, installation, cancellation and existing installed games retain their mainline behavior

#### Scenario: Registration is removed and added again
- **WHEN** the user removes a port registration and later adds the same executable
- **THEN** its files and provenance survive removal and the existing installation is relinked under mainline rules

#### Scenario: Application runs on Linux
- **WHEN** the integrated application runs on Linux
- **THEN** PC ports keeps its existing Windows-only installation availability and Steam plugin integration does not expose unsupported Linux installation controls
