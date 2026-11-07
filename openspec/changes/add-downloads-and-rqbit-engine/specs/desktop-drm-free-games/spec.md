# Spec Delta

## ADDED Requirements

### Requirement: Entries created from completed downloads
An accepted automatic install on Windows SHALL create one DRM-free entry with a stable installation identity, the detected executable, provenance naming the transfer info hash, and background matching. It SHALL follow the same editable settings, repair, removal and duplicate rules as a manually added entry. Removing it SHALL NOT delete the installed files or the transfer.

#### Scenario: Entry from a download
- **WHEN** the user accepts the detected result of a completed transfer
- **THEN** a playable DRM-free entry appears with provenance to the transfer and matching begins in the background

#### Scenario: Duplicate executable
- **WHEN** the detected executable is already registered through any path spelling
- **THEN** the existing entry is selected instead of creating a second installation

#### Scenario: Remove a downloaded entry
- **WHEN** the user removes a downloaded entry from the library
- **THEN** the registration leaves the library while the transfer and the installed files remain on disk

#### Scenario: Provenance in details
- **WHEN** a downloaded entry is selected
- **THEN** its details identify the transfer it came from

### Requirement: Installer execution is explicit and outside discovery
Inspecting a completed transfer or watching a folder SHALL NOT execute anything. An installer SHALL run only through the automatic installation flow after the per-install confirmation defined by `desktop-download-install`.

#### Scenario: Discovery never executes
- **WHEN** a watched folder or a completed transfer contains an installer
- **THEN** inspection reads metadata only and runs no executable

#### Scenario: Explicit installation
- **WHEN** the user confirms the installer in the automatic installation flow
- **THEN** exactly that installer runs once for the job
