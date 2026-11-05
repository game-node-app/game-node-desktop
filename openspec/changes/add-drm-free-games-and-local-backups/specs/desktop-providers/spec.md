# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: Built-in DRM-free provider
Windows SHALL include a bundled manual provider whose scan reads approved local registrations, whose display name is DRM-free and whose playtime source is `manual`. Its ids SHALL be installation identities rather than catalog or manifest ids. Unapproved folder discoveries SHALL never be scanner records. It SHALL not declare a store counter.

#### Scenario: Scan manual registrations
- **WHEN** a scan runs after an executable was manually added
- **THEN** its `manual:<installation id>` entry appears alongside Steam entries without requiring another picker interaction

#### Scenario: GameNode identity changes
- **WHEN** the GameNode match or Ludusavi profile changes
- **THEN** the provider id, installation id and `manual` playtime source stay unchanged
