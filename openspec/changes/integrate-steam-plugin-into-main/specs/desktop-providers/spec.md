# Spec Delta

## MODIFIED Requirements

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

#### Scenario: Unknown playtime counter
- **WHEN** a manifest names a playtime counter the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the counter

#### Scenario: Unknown catalog backend
- **WHEN** a manifest names a catalog backend that is neither available natively nor authorized through its registered plugin
- **THEN** that manifest is rejected with a diagnostic naming the backend and other providers remain usable

#### Scenario: Undeclared executable action
- **WHEN** a non-manual provider declares an executable action
- **THEN** it is rejected and no executable can be launched through that declaration

#### Scenario: Valid plugin backend
- **WHEN** a manifest selects a scanner, catalog or counter capability from its registered plugin descriptor
- **THEN** the host accepts that operation without requiring a Steam-specific native backend

#### Scenario: Plugin capability exceeds its descriptor
- **WHEN** the manifest requests an operation or permission not approved for that plugin
- **THEN** the host rejects it and other valid providers remain usable

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

## ADDED Requirements

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

### Requirement: Steam web sessions survive the access cookie lifetime
The Steam plugin SHALL persist the long-lived `steamRefresh_steam` cookie captured from the trusted login window alongside the store access cookie and SHALL renew an expired `steamLoginSecure` through `login.steampowered.com` before the account is considered disconnected. A renewal SHALL be persisted before its follow-up validation, and concurrent session validations SHALL be serialized so a rotated refresh cookie cannot be lost.

#### Scenario: Access cookie expires after a day
- **WHEN** the store page rejects the saved access cookie but the saved refresh cookie is accepted
- **THEN** the plugin stores the renewed cookies and reconnects without asking the user to sign in

#### Scenario: Remember me was not selected
- **WHEN** the login window produced no refresh cookie
- **THEN** the account becomes expired at the normal access-cookie expiry and the UI asks for a new sign-in

### Requirement: Steam credentials are removed only on definitive rejection
The plugin SHALL remove a saved Steam credential only when the store session is rejected and no renewal is possible. A catalog API rejection SHALL NOT delete a credential whose store session validated.

#### Scenario: Refresh cookie rejected
- **WHEN** the login host rejects the saved refresh cookie
- **THEN** the account becomes expired, the dead credential is removed and cached library facts stay visible

#### Scenario: Catalog token rejected after a valid store check
- **WHEN** the owned-games API rejects its short-lived token
- **THEN** the catalog reports expired without deleting the saved refresh cookie

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
