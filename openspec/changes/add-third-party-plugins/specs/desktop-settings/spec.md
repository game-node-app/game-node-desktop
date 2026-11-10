# Spec Delta

## ADDED Requirements

### Requirement: Third-party plugin management in Stores
The Stores panel SHALL extend its plugin inventory with separate official and third-party attribution and actions to import, enable, disable, review permissions, check/review updates and remove third-party packages. It SHALL show active version, publisher fingerprint, compatibility and failure status without adding a new navigation topic.

#### Scenario: Import example plugin
- **WHEN** the user selects Install plugin and chooses a reference package
- **THEN** the panel shows package validation progress and opens a review containing identity, publisher, license and requested permissions before activation

#### Scenario: Official plugin selected
- **WHEN** the user selects Steam in the inventory
- **THEN** its existing official attribution/details remain and third-party permission or update controls cannot alter its grants or bundled binary

### Requirement: Effective access review
Plugin details SHALL distinguish requested, granted and effective API permissions, their scopes and read/write/execute effects. Native-access declarations SHALL be identified as publisher statements rather than enforced OS restrictions. First activation and changed native declarations SHALL require explicit acknowledgement of native-code risks and independent maintenance.

#### Scenario: Review file modification capability
- **WHEN** the user reviews a plugin requesting filesystem writes or game/tool installation
- **THEN** the UI identifies the resources it can change through the API and explains that direct OS activity may exceed those grants

#### Scenario: Deny optional permission
- **WHEN** the user denies an optional grant
- **THEN** the saved effective state and affected unavailable features appear without describing the plugin as sandboxed

### Requirement: Configure delegates functional configuration
The inventory SHALL offer Configure for enabled compatible plugins with an authorized configuration mode. It SHALL prefer a declared plugin-owned window, focus an existing one and use a declarative form only as a separately supported option. Installation, grants, activation, updates and licensing SHALL remain administered in GameNode.

#### Scenario: Rich plugin configuration
- **WHEN** the user activates Configure for a plugin with external configuration
- **THEN** GameNode requests its own window to open or focus and does not require its controls to fit a JSON form

#### Scenario: Plugin is disabled
- **WHEN** the user inspects a disabled plugin
- **THEN** its management controls remain accessible but Configure cannot start plugin code until activation is approved

#### Scenario: No available configuration mode
- **WHEN** a plugin declares no supported and authorized configuration mode
- **THEN** Configure is unavailable with an explanation and the host does not launch an arbitrary fallback URL or executable

### Requirement: Offline plugin license notices
The inventory SHALL display each plugin's declared license and complete bundled notices offline, with publisher/source links when supplied. It SHALL distinguish plugin license terms from the Core GPL license and public SDK MIT terms, without claiming verified legal compliance.

#### Scenario: Proprietary license review
- **WHEN** an installed proprietary plugin is selected without network access
- **THEN** its bundled terms remain readable and Core licensing information remains accurate

### Requirement: Update review and recovery status
The inventory SHALL show installed/candidate versions, compatibility and changes in permissions, scopes, license and native access before approving an update. Verification, activation and rollback failures SHALL have actionable localized states; a failed update SHALL show the actual active version.

#### Scenario: Rollback completes
- **WHEN** an update fails and the previous verified version is restored
- **THEN** the UI shows that restored version, the failure reason and the current unchanged permission decisions

### Requirement: Responsive and accessible plugin controls
Plugin management SHALL keep disk/network/process work off the UI thread, expose progress and prevent duplicate submissions while preserving unrelated interaction. Controls, confirmations, errors and risk disclosures SHALL support keyboard access and English/pt-BR text using the application's shared modal and focus behavior.

#### Scenario: Slow signature verification
- **WHEN** package verification takes time
- **THEN** the user can continue navigating settings while the affected action shows progress

#### Scenario: Permission update fails
- **WHEN** persistence of a permission change fails
- **THEN** the control returns to the actual saved state and an accessible localized error is shown

### Requirement: Per-game launch recipe review and selection
GameNode SHALL let users review and select one approved custom recipe per game/profile or the detected default. Review SHALL identify the contributing plugin, executable/URI, runtime, arguments, working directory and environment effects. Changes requiring new approval SHALL be visible; permission revocation SHALL leave the detected default available as an explicit choice.

#### Scenario: Choose an extension's Proton recipe
- **WHEN** the user reviews a custom recipe for a selected Steam, third-party or local game
- **THEN** its runtime and effective parameters are shown before approval and selection for Play

#### Scenario: Selected extension is disabled
- **WHEN** the selected recipe depends on a disabled or revoked plugin
- **THEN** the UI explains why that route is unavailable and offers an explicit choice of the detected default without silently starting it

### Requirement: Data sharing source and recipient review
Settings SHALL show sharing disabled by default and let users select the exact source/recipient plugin, developer, origin, publisher identity and installed version, plus fields/resources, snapshot/live delivery, history and persistence. No preselected option SHALL silently grant a new field, recipient or version.

#### Scenario: Authorize hardware fields
- **WHEN** the user selects GPU model and RAM capacity for a chosen consumer
- **THEN** only those fields from the selected source are shared and CPU/identifiers remain excluded

### Requirement: Persistence usage and cleanup administration
Settings SHALL show plugin-attributed managed usage, quota, retention and user-visible physical location, with cleanup preview and uninstall keep/delete choices. Plugin APIs SHALL not receive those physical paths. The UI SHALL distinguish archived data from installed code and avoid suggesting retained data is automatically shared.

#### Scenario: Inspect retained data
- **WHEN** the user opens storage administration for an uninstalled plugin
- **THEN** the UI shows its archived size/location and explicit reuse/delete choices while plugin-facing APIs keep the location hidden

### Requirement: Unavailable storage and save scope review
GameNode SHALL report unavailable approved game/save/runtime volumes without deleting their registrations. Save-profile changes and shared emulator resources SHALL show affected paths and games before authorization. Storage media hints SHALL not be presented as a conclusive performance diagnosis.

#### Scenario: Missing external save disk
- **WHEN** a registered volume disconnects
- **THEN** the user sees an unavailable-volume state and previous save profiles/history remain intact
