# Spec Delta

## ADDED Requirements

### Requirement: Store detection reports its location
A provider's scan MAY report where the store is installed and how many library folders it uses. When reported, the host SHALL keep the latest values with the scan result and show them in Settings. A provider that does not report them SHALL be shown with its installed count only. The values SHALL be informational and SHALL NOT change which folders are scanned.

#### Scenario: Steam with several libraries
- **WHEN** the Steam scan reads `libraryfolders.vdf` under `C:\Program Files (x86)\Steam` with four library folders
- **THEN** the scan result reports that path and four library folders

#### Scenario: Provider without location
- **WHEN** a provider's scan succeeds without reporting a location
- **THEN** Settings shows the store as detected with its installed count and no path

#### Scenario: Store not installed
- **WHEN** a provider's scan finds no installation
- **THEN** no location is reported and Settings shows the store as not found
