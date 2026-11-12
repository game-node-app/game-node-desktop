# Spec Delta

## ADDED Requirements

### Requirement: Providers declare shared catalog access
A provider manifest with a catalog MAY declare shared access with a non-empty display label of at most 32 characters and no control characters, such as Steam's `Steam Family`. A manifest that declares shared access without a catalog, or with an invalid label, SHALL be rejected like any other invalid manifest. The label is a proper name: it SHALL be shown as declared and SHALL NOT be translated.

#### Scenario: Steam declares its family label
- **WHEN** the bundled manifests load
- **THEN** Steam declares shared access labelled `Steam Family` and Epic declares no shared access

#### Scenario: Shared access without a catalog
- **WHEN** a manifest declares shared access but no catalog
- **THEN** that manifest is rejected with a diagnostic and the other providers still load

#### Scenario: Invalid label
- **WHEN** a manifest declares an empty shared label, a label longer than 32 characters, or a label with a control character
- **THEN** that manifest is rejected with a diagnostic

### Requirement: Shared catalog data requires declared shared access
The host SHALL accept shared catalog records and shared catalog failures only from a provider whose manifest declares shared access. If a provider without that declaration sends shared data, the sync SHALL fail as a protocol error, and the previous facts SHALL be kept.

#### Scenario: Undeclared shared records
- **WHEN** a provider without declared shared access streams shared catalog records
- **THEN** the sync fails as a protocol error and the previously owned and shared facts stay unchanged

#### Scenario: Declared shared records
- **WHEN** Steam streams shared catalog records
- **THEN** they are accepted as shared facts for Steam entries

### Requirement: Stored shared failures survive the rename
A shared-catalog failure SHALL be recorded under a provider-neutral category. Categories stored by earlier builds under the family name SHALL still be read as the same failure.

#### Scenario: Failure stored by an older build
- **WHEN** the database holds a shared-catalog failure written by a build that used the family category name
- **THEN** Stores shows the same shared-catalog warning as for a failure written by this build

## MODIFIED Requirements

### Requirement: Independent installed, owned and family facts
The library SHALL represent installed, owned, and shared as independent facts for the same provider entry. The shared fact covers games the account can play through the provider's declared shared access, such as Steam Family, without owning them. An entry SHALL be visible when at least one fact is true. A completed installed scan, owned sync, or shared sync SHALL update only its own fact set.

#### Scenario: Catalog-only game
- **WHEN** a game is owned but not installed
- **THEN** its entry is visible with an empty install folder

#### Scenario: Uninstalled but owned
- **WHEN** an owned game is uninstalled and a later installed scan no longer finds it
- **THEN** the entry stays visible as owned and is no longer installed

#### Scenario: Installed but not owned
- **WHEN** an installed Steam game is not in the owned or shared catalogs
- **THEN** the entry stays visible as installed

#### Scenario: Failed installed scan
- **WHEN** the installed scan fails
- **THEN** installed facts are unchanged, including entries that were installed before the failure

#### Scenario: Provider without shared access
- **WHEN** a provider that declares no shared access completes a catalog sync
- **THEN** none of its entries is shared and only its owned facts change
