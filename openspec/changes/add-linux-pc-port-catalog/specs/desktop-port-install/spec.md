## Purpose

Define the Linux installation boundary for PC ports. Phase 1 keeps Linux read-only; phase 2 adds
native Linux installs without executing Windows-only assets or tools.

## ADDED Requirements

### Requirement: Phase 1 Linux catalog is read-only

Before native Linux installs are enabled, the host SHALL refuse every PC port install on Linux
without downloading a release, fetching an extractor, starting a process or registering a library
entry. The UI SHALL expose the refusal as the Coming Soon state, not a generic error.

#### Scenario: Install is requested on Linux in phase 1

- **WHEN** an install command is invoked for any catalog entry on Linux in phase 1
- **THEN** the host refuses before any network, filesystem or process work and the UI keeps the
  entry browsable

### Requirement: Native Linux installs use Linux assets and XDG roots

When native Linux installs are enabled, the host SHALL select a verified Linux asset from the
reviewed release, normalize its executable permission where required, place the files under the
XDG `app`, `data`, `config` and `localData` roots according to the recipe, and register the result
as an ordinary DRM-free library entry with the same provenance and playtime model. Windows-only
assets SHALL be rejected before download.

#### Scenario: Linux AppImage release

- **WHEN** the selected release contains a verified Linux AppImage and the recipe has no
  Linux-unavailable requirement
- **THEN** the host installs it under the chosen XDG root, makes the file executable, validates the
  launcher and registers the DRM-free entry

#### Scenario: Windows-only release

- **WHEN** the selected release contains only Windows assets
- **THEN** the host refuses the Linux install before downloading and reports the Windows-only
  reason

### Requirement: Windows-only recipes are never executed on Linux

A Linux install SHALL NOT fetch or execute the Windows PS4 PKG extractor, start Wine/Proton, or
substitute a Windows executable. A recipe that requires `from_package` with the Windows extractor
SHALL be reported unavailable on Linux until a reviewed Linux tool and recipe exist.

#### Scenario: Recipe requires console extraction

- **WHEN** a Linux catalog entry's only recipe requires the Windows PS4 PKG extractor
- **THEN** the entry remains blocked with an explicit unavailable reason and no extractor is
  fetched or run

#### Scenario: Future Proton path

- **WHEN** Proton/Wine support is proposed later
- **THEN** it is delivered by a separate change; this change does not install or launch Windows
  executables on Linux
