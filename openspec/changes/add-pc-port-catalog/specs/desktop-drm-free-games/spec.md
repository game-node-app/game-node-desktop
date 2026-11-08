# Spec Delta

## ADDED Requirements

### Requirement: Port installs create DRM-free entries
A completed port install SHALL create a DRM-free entry with the same identity, launch, tracking, playtime, backup, repair and removal behaviour as an entry added by hand. The entry SHALL record its port provenance: catalog id, port name, installed release version and install folder. It SHALL be listed under the DRM-free provider.

#### Scenario: Port entry behaves like a manual entry
- **WHEN** a port install completes and the user opens the entry's settings
- **THEN** its title, folder, executable, working directory, arguments and tracked executables are editable as for any DRM-free entry

#### Scenario: Provenance is shown
- **WHEN** the user selects a port-installed entry in the library
- **THEN** the detail pane shows the port name and installed version with a link to its PC ports entry

### Requirement: Port entries launch through the launcher and track gameplay
When the port overlay names a launcher executable and gameplay executables, the entry SHALL launch the launcher and SHALL track play time while a gameplay executable runs. When it names neither, the registered executable SHALL be both launched and tracked. Recipe arguments SHALL be prepended to any arguments the user adds.

#### Scenario: Launcher starts the game
- **WHEN** the user plays a Lantern Engine entry, the launcher starts, and the launcher then starts `Bloodborne.exe`
- **THEN** the session is attributed to the entry while `Bloodborne.exe` runs

#### Scenario: Recipe argument and user argument
- **WHEN** a recipe passes `--iso <path>` and the user adds `--fullscreen`
- **THEN** the port starts with `--iso <path> --fullscreen`

### Requirement: Removing a port entry keeps its files
Removing a port-installed entry SHALL follow the same rules as removing any DRM-free entry: it SHALL remove only the registration and SHALL NOT delete the install folder, copied game files, saves or backups. The removal confirmation SHALL show the install folder with an Open folder action. Watched-folder discovery SHALL NOT suggest the removed port again until reconsidered.

#### Scenario: Remove a port
- **WHEN** the user removes a port-installed entry and confirms
- **THEN** the entry disappears from the library, the install folder and its copied dump remain on disk, and PC ports shows the entry as not installed
