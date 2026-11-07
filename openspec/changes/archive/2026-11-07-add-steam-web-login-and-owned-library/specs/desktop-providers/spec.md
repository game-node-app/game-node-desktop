# Spec Delta

## ADDED Requirements

### Requirement: Steam catalog capability
A manifest MAY declare `catalog = { native = "<name>" }`. Only catalog backends built into the
application are valid. A provider without a catalog has no account connection or owned-library
import. Catalog records SHALL carry a provider-local id and a non-empty name and MAY be marked
family-shared; the manifest's `exclude_ids` SHALL apply to them.

#### Scenario: Steam manifest declares a catalog
- **WHEN** the bundled Steam manifest declares the Steam catalog backend
- **THEN** Steam can list owned and family-shared games in addition to installed games

#### Scenario: Empty catalog record
- **WHEN** a catalog response contains an entry without a usable id or without a name
- **THEN** that entry is dropped and logged and the rest of the catalog is kept

#### Scenario: Excluded app
- **WHEN** an owned or family-shared app id appears in the manifest's `exclude_ids`
- **THEN** it does not become a library entry

### Requirement: Steam owned catalog
With a valid Steam session, the Steam catalog SHALL read every game the account owns, including
uninstalled, unplayed, and free games, using the account's short-lived web API token. It SHALL keep
numeric app ids with non-empty names, remove duplicates, and treat an empty library as a
successful empty catalog. Transient Steam responses SHALL be retried a bounded number of times.

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
When the account belongs to a Steam Family, the catalog SHALL read the games shared with it and
treat them as visible Steam entries distinct from owned games. An account in no family SHALL
produce no family-shared games without an error. A family-shared game that the account also owns
SHALL be treated as owned only.

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
A failed owned catalog fetch SHALL leave the previous owned facts unchanged. A failed family fetch
SHALL leave the previous family facts unchanged and SHALL NOT fail the owned catalog or the
installed scan. A completed sync SHALL replace only the fact set it covers.

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
The library SHALL represent installed, owned, and family-shared as independent facts for the same
provider entry. An entry SHALL be visible when at least one fact is true. A completed installed
scan, owned sync, or family sync SHALL update only its own fact set.

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
A catalog entry SHALL use the same `<provider>:<store id>` key as an installed entry for the same
app. It SHALL preserve the entry's first-seen time, chosen match, automatic match, cover, and
playtime history when facts change or the entry is temporarily invisible. Clearing ownership SHALL
NOT delete entries.

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
A catalog sync SHALL be requested for every scan of a provider with a catalog and when its account
becomes connected. An explicit rescan or Stores refresh SHALL request it too. Only one catalog sync
per provider SHALL run at a time, and requests made while one runs SHALL coalesce.

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

## MODIFIED Requirements

### Requirement: Manifest validation isolates failures
The application SHALL validate every manifest when it loads. Validation SHALL reject unknown fields, missing required fields, an unknown native scanner, an unknown catalog backend, an unknown playtime counter, a duplicate provider id, malformed placeholders, and invalid action declarations. Executable launch SHALL be restricted to the bundled manual provider; other providers SHALL retain declared URI actions. An invalid manifest SHALL be skipped and logged, and the other manifests SHALL still load.

#### Scenario: One manifest is invalid
- **WHEN** one bundled manifest fails validation
- **THEN** it is skipped with a diagnostic naming the provider and the problem
- **AND** every valid provider still scans and launches

#### Scenario: Unknown native scanner
- **WHEN** a manifest names a native scanner the application does not have
- **THEN** that manifest is rejected and no scan runs for it

#### Scenario: Unknown catalog backend
- **WHEN** a manifest names a catalog backend the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the backend and no catalog sync runs for it

#### Scenario: Unknown playtime counter
- **WHEN** a manifest names a playtime counter the application does not have
- **THEN** that manifest is rejected with a diagnostic naming the counter

#### Scenario: Undeclared executable action
- **WHEN** a non-manual provider declares an executable action
- **THEN** it is rejected and no executable can be launched through that declaration
