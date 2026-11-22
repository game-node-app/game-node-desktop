## ADDED Requirements

### Requirement: Provider roots and launcher actions are platform-specific

A provider manifest MAY declare different roots and action behavior per platform. The host SHALL
select only the roots for the running platform and SHALL NOT add store-specific code. The Epic
provider SHALL keep its Windows registry/ProgramData roots and add Linux roots for the native
launcher without changing the Windows path.

#### Scenario: Linux Epic roots

- **WHEN** the Epic provider loads on Linux
- **THEN** only its Linux roots are resolved and forwarded to the plugin, and the Windows roots are
  ignored

#### Scenario: Windows Epic roots

- **WHEN** the Epic provider loads on Windows
- **THEN** its registry/ProgramData roots and existing launch/install URI behavior are unchanged
