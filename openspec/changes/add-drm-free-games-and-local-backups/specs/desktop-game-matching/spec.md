# Spec Delta

## ADDED Requirements

### Requirement: Infer a title before asking for a match
For DRM-free entries, the application SHALL attempt title inference from executable product metadata, meaningful executable names and installation context, enriched by manifest evidence when available. Generic engine, launcher and maintenance names SHALL not establish an identity. It SHALL retain the detected title separately from the catalog title and allow the user to edit it.

#### Scenario: Product metadata identifies the game
- **WHEN** `game.exe` has a meaningful product title supported by its installation context
- **THEN** that title is used for the first catalog lookup without asking the user to type it

#### Scenario: Useful executable name
- **WHEN** `Celeste.exe` has no useful product metadata
- **THEN** its meaningful filename is tried as title evidence and the entry remains playable during lookup

#### Scenario: Generic binary name
- **WHEN** a binary is named `Game-Win64-Shipping.exe` and its folder gives no reliable game identity
- **THEN** the name does not cause a speculative automatic match and manual search remains available

### Requirement: Automatic local-evidence matches are conservative
The first DRM-free matching pass SHALL automatically select a unique GameNode identity only when an evidenced external id maps uniquely or a meaningful evidenced title matches uniquely after conservative normalization. It SHALL preserve sequel numbers and edition distinctions. Conflicting signals, ambiguous results, fuzzy-only similarity and incomplete lookup results SHALL remain unmatched suggestions.

#### Scenario: Manifest external identity
- **WHEN** a strongly identified manifest game has a Steam id that GameNode maps to one game
- **THEN** that identity can match the manual entry without changing its provider, playtime source or launch behavior

#### Scenario: Unique exact title
- **WHEN** executable evidence gives `Celeste` and a completed search returns one exact title match
- **THEN** the entry is matched automatically and its cover is requested

#### Scenario: Edition ambiguity
- **WHEN** the evidence does not distinguish an original game from a remaster, or a sequel from its predecessor
- **THEN** no automatic match is chosen and the user can select a suggested result

#### Scenario: Several exact results or truncated search
- **WHEN** several GameNode identities share the evidenced title or search completeness cannot be established
- **THEN** the application does not select the first returned result automatically

### Requirement: Local matching honors choices and stale-result boundaries
Local-evidence matching SHALL obey existing account-specific chosen-match and no-match overrides. It SHALL retain valid cached matches on lookup failure and ignore results for superseded executable evidence, removed entries or previous accounts. Match provenance and a way to change it SHALL be visible in details.

#### Scenario: Wrong first attempt
- **WHEN** an automatic match is wrong and the user selects another game with Change match
- **THEN** subsequent title inference, rescans and manifest updates retain that choice

#### Scenario: Explicit no-match
- **WHEN** the user marks the entry as having no GameNode game
- **THEN** automatic matching does not resume unless that choice is cleared

#### Scenario: Slow lookup after repair
- **WHEN** an executable is repaired while an earlier lookup is still running
- **THEN** its old result cannot replace the repaired entry's current match
