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

### Requirement: Actionable platform compatibility review
Import/details and update review SHALL show the target, mandatory requirements, detected effective environment and typed verdict/reasons. Author-tested coverage SHALL be labelled separately from eligibility. Incompatible or indeterminate mandatory checks SHALL prevent activation/Configure while leaving management/removal available. Unknown facts SHALL remain labelled unknown; compatible SHALL NOT imply safe code or successful execution.

#### Scenario: Wrong libc family
- **WHEN** a plugin requires glibc but the effective environment uses musl
- **THEN** the UI identifies both families and the unavailable activation reason instead of showing only a generic failure

#### Scenario: Environment not author-tested
- **WHEN** a target passes mandatory checks but has no matching tested-environment entry
- **THEN** the UI distinguishes that informational state from incompatibility and applies normal activation consent

#### Scenario: Required fact is unknown
- **WHEN** a mandatory environment fact cannot be determined
- **THEN** the UI reports indeterminate with required/unknown values, keeps activation and Configure unavailable and leaves management/removal accessible

### Requirement: Effective access review
Plugin details SHALL distinguish requested, granted and effective API permissions, their scopes and read/write/execute effects. Native-access declarations SHALL be identified as publisher statements rather than enforced OS restrictions. First activation and changed native declarations SHALL require explicit acknowledgement of native-code risks and independent maintenance.

#### Scenario: Review file modification capability
- **WHEN** the user reviews a plugin requesting filesystem writes or game/tool installation
- **THEN** the UI identifies the resources it can change through the API and explains that direct OS activity may exceed those grants

#### Scenario: Deny optional permission
- **WHEN** the user denies an optional grant
- **THEN** the saved effective state and affected unavailable features appear without describing the plugin as sandboxed

### Requirement: Creator and selected foreign resource scope review
Domain permission controls SHALL independently present read/write/delete, own-creator scope and explicitly selected foreign runtime/library-root/save-profile records. Review SHALL identify creator and acting plugin/version, resources and allowed effects. All grants SHALL start denied. Removing a runtime registration SHALL disclose affected bindings without implying game-file, save or history deletion.

#### Scenario: Extend one runtime scope
- **WHEN** the user grants a migration plugin read/write access to one foreign runtime but leaves delete denied
- **THEN** only those operations and that resource are added to its scope, while unrelated resources and private namespaces remain inaccessible

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

### Requirement: Shared-data permission and review controls
Settings SHALL show common-data category read permissions granted during plugin activation, including their resource scope and whether they include history. These grants SHALL permit explicit per-operation queries across all publishers in the category without pairwise approval and SHALL NOT imply automatic plugin use. Sensitive/custom sharing SHALL retain source/recipient, field/resource, delivery, history and retention review. Secret data SHALL remain excluded from both flows.

#### Scenario: Review common performance access
- **WHEN** the user reviews a plugin's granted performance-history category permission
- **THEN** Settings shows that the plugin may explicitly query matching history from any publisher within its granted scope and whether recipient persistence is separately allowed

#### Scenario: Review common content-catalog access
- **WHEN** the user reviews a plugin's granted common content-catalog permission
- **THEN** Settings explains that an explicit query can return ROM/content availability from other runtimes in scope without granting access to ROM files or bytes

### Requirement: Persistence usage and cleanup administration
Settings SHALL show plugin-attributed managed usage, quota, retention and user-visible physical location, with cleanup preview and uninstall keep/delete choices. Plugin APIs SHALL not receive those physical paths. The UI SHALL distinguish archived data from installed code and explain that access to retained data follows the consumer's granted category permissions or sensitive-data review.

#### Scenario: Inspect retained data
- **WHEN** the user opens storage administration for an uninstalled plugin
- **THEN** the UI shows its archived size/location and explicit reuse/delete choices while plugin-facing APIs keep the location hidden

### Requirement: Unavailable storage and save scope review
GameNode SHALL report unavailable approved game/save/runtime volumes without deleting their registrations. Save-profile changes and shared emulator resources SHALL show affected paths and games before authorization. Storage media hints SHALL not be presented as a conclusive performance diagnosis.

#### Scenario: Missing external save disk
- **WHEN** a registered volume disconnects
- **THEN** the user sees an unavailable-volume state and previous save profiles/history remain intact

### Requirement: Common catalog and sensitive sharing controls
Common runtime and other catalog permissions SHALL let users grant category-scoped reads while showing provenance and available metadata projections. Sensitive/custom fields SHALL remain under explicit sharing review. New categories or scopes SHALL not silently become visible or trigger new Core behavior.

#### Scenario: Grant runtime lookup
- **WHEN** the user grants B the applicable runtime category read permission
- **THEN** B can discover matching records from any publisher without a separate send-to-B action and the UI still distinguishes private fields

### Requirement: Unified launch-plan review
Every GameNode-mediated execution customization SHALL use a host-generated launch-plan review, including runtimes/emulators, known mod loaders/layers, alternate targets, arguments, cwd and environment changes. The review SHALL show game, source/developer/origin/version, effective programs/URI, ordered intermediaries, full arguments and changes from the default/previous plan. Plugin explanations SHALL be labelled separately from host facts.

#### Scenario: Runtime customization
- **WHEN** the user first selects an emulator/runtime-based customized launch
- **THEN** the review displays the resolved runtime, game/content target, source and effective parameters

#### Scenario: Modded launch
- **WHEN** a known mod loader or declared launch-time mod layer changes the execution plan
- **THEN** the same review identifies those components and changes before approval

#### Scenario: Environment-only customization
- **WHEN** a customization changes only environment values or working directory
- **THEN** the host still presents the effective change rather than skipping review because the executable is unchanged

### Requirement: Scoped launch consent choices
Launch review SHALL offer Cancel as the default, Run once and optional remembered approval for this recipe/game. No choice SHALL grant global execution permission. An unchanged approved plan SHALL run through user Play without another prompt. Changed plan-bound components, executable identity, arguments, cwd, environment or authorization SHALL require renewed approval.

#### Scenario: Run once
- **WHEN** the user selects Run once
- **THEN** only that displayed plan receives an expiring single-use approval

#### Scenario: Remembered recipe changes
- **WHEN** a mod loader, runtime, plugin version or effective argument changes after remembered approval
- **THEN** the prior approval cannot execute the changed plan without another review

### Requirement: Honest launch disclosure and complete details
Launch review SHALL state that scope validation does not certify native executable behavior. It SHALL show complete execution details safely as literal text, with unknown facts identified and secret values revealable only explicitly. Forbidden API requests SHALL not offer Execute anyway. Review SHALL support keyboard access, English and pt-BR.

#### Scenario: Forbidden request
- **WHEN** a launch request requires a prohibited shell/elevation route or unauthorized scope
- **THEN** the host refuses it before confirmation and provides no consent bypass

#### Scenario: Plugin claims launch is safe
- **WHEN** a plugin provides a reassuring explanation
- **THEN** the UI labels it as plugin-provided text and retains the independent host disclosure
