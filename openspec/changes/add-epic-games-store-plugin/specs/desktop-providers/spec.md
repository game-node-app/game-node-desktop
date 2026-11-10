# Spec Delta

## ADDED Requirements

### Requirement: Store integrations are delivered only by plugins
Every store provider SHALL be implemented by an official plugin and reach Core only through the public plugin contract and scoped host services. Host crates SHALL NOT contain production code that interprets store files, store HTTP responses or store login data, and SHALL NOT branch on a store provider id. The manual DRM-free provider is the only built-in provider.

#### Scenario: Adding a store
- **WHEN** a new store provider is added
- **THEN** it needs a plugin, a provider manifest and a trusted descriptor, and no store-specific Rust code in Core, API, platform or application host crates

#### Scenario: Plugin unavailable
- **WHEN** a store's plugin cannot run
- **THEN** that provider is unavailable, and there is no in-process fallback that reads the store directly

#### Scenario: Boundary regression
- **WHEN** production host code gains a reference to a store-specific module, host name or provider-id branch outside trusted descriptor data and test fixtures
- **THEN** the automated boundary check fails

### Requirement: Matching may use a declared record field
A manifest's match declaration SHALL optionally name a record field to use as the external id instead of the provider id. Lookups and cached automatic matches SHALL then use that field's value. Entries without the field SHALL NOT be looked up. Without the declaration, behavior SHALL be unchanged.

#### Scenario: Steam unchanged
- **WHEN** the Steam manifest declares no match field
- **THEN** Steam entries are looked up by app id exactly as before

#### Scenario: Field-based lookup
- **WHEN** a manifest declares `uid_field = "namespace"` and an entry has that field
- **THEN** the lookup sends the field's value, and the answer matches every entry of that provider carrying the same value

#### Scenario: Catalog entry fields
- **WHEN** a plugin reports an owned game with named fields
- **THEN** the fields are stored on the entry and remain available to matching and action templates after restarts

### Requirement: Store data in host features is keyed by entry key
Host features that need per-store facts, such as the game-tools anti-cheat blocklist, SHALL read them from bundled data keyed by the full provider entry key. They SHALL NOT parse store-specific id prefixes in code.

#### Scenario: Blocked Steam title
- **WHEN** the tools blocklist data lists `steam:1172470`
- **THEN** that entry is blocked exactly as before the change

#### Scenario: Blocked Epic title
- **WHEN** the blocklist data lists an `epic:<appName>` key
- **THEN** that Epic entry is blocked without any code change
