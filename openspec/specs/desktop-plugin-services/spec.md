# desktop-plugin-services Specification

## Purpose

Expose explicit provider-scoped host services that let the Steam plugin own its integration while GameNode retains application authority, protected persistence and platform resource ownership.

## Requirements

### Requirement: Host services require explicit grants
Each host service SHALL require its named grant and validate plugin, session, operation, the applicable provider or runtime-content context and applicable profile/account generation. A denied call SHALL return no protected data and apply no side effect. Private resources of another plugin SHALL remain inaccessible. Explicit public-pool category or selected-record operation grants SHALL authorize only eligible shared resources, never foreign credentials or private namespaces.

#### Scenario: Missing permission
- **WHEN** a session without secret or login grants requests those services
- **THEN** the host rejects it without reading credentials or opening a window

#### Scenario: Cross-plugin handle
- **WHEN** a plugin presents another plugin's resource handle
- **THEN** access is rejected unless an explicit shared-resource operation grant covers that handle; matching permission names alone do not authorize access

### Requirement: Provider context excludes application secrets
`provider.context.read` SHALL expose only validated configuration, platform, negotiated limits and operation context belonging to the caller's provider. It SHALL exclude GameNode tokens, unrelated provider configuration, raw database access and internal commands.

#### Scenario: Steam initializes
- **WHEN** Steam requests its authorized initialization context
- **THEN** it receives its own provider configuration and public operational limits without GameNode credentials

### Requirement: Discovery permission submits installed observations
`library.discovery` SHALL accept installed-game observations only for the current authenticated provider operation or a host-issued scoped runtime-content operation. Success with no games SHALL be distinct from failure. Validated observations SHALL enter existing reconciliation rather than granting direct library mutation. Runtime-content contexts SHALL bind plugin identity and approved roots without provider impersonation or authority over foreign provider facts.

#### Scenario: Steam is absent
- **WHEN** discovery succeeds without finding a Steam installation
- **THEN** the host applies a successful empty installed result

#### Scenario: Discovery fails
- **WHEN** the plugin reports a scan-level failure
- **THEN** the previous installed facts remain and a categorized error is shown

#### Scenario: Extension discovers runtime content
- **WHEN** an authorized extension without a provider submits content observations under a current host-issued runtime-content operation and approved roots
- **THEN** Core reconciles content and candidate runtime bindings without granting a provider account context or executing content

#### Scenario: Discovery context missing
- **WHEN** discovery is submitted without either a valid provider operation or a valid scoped runtime-content operation
- **THEN** the host rejects it without modifying successful discovery facts

### Requirement: Catalog permission represents account ownership
`library.catalog` SHALL accept complete owned and Family fact sets for the authorized account operation. Each category SHALL distinguish complete, empty, failed and unchanged results. Partial or stale results SHALL NOT replace successful facts. This grant SHALL NOT authorize a browsable PC ports catalog or installation work.

#### Scenario: Family fails independently
- **WHEN** owned results succeed and Family results fail
- **THEN** owned facts update while applicable previous Family facts remain with a separate warning

#### Scenario: Valid category is empty
- **WHEN** the current account successfully completes a category with no games
- **THEN** its previous facts for that category are cleared

#### Scenario: Plugin requests port installation
- **WHEN** a plugin with only `library.catalog` requests installation of a release asset
- **THEN** that request is not authorized by the ownership grant

### Requirement: Account services exchange non-secret state
`account.state.read` SHALL expose the caller's saved non-secret account metadata. `account.state.publish` SHALL accept typed identity, connection state, timestamps and categorized failures for authorized transitions. Secrets SHALL NOT enter GUI state, and persistence indicators SHALL reflect actual service outcomes.

#### Scenario: Restore saved account
- **WHEN** the plugin reads its identity and validates its saved session
- **THEN** it can publish connected state for the generic account controls

#### Scenario: Credential persistence fails
- **WHEN** login succeeds but the secret cannot be saved
- **THEN** the GUI reports session-only connection without claiming successful persistence

### Requirement: Account transitions reject obsolete side effects
Cancel, sign-out, account replacement and profile changes SHALL invalidate obsolete operations. Authorization SHALL be rechecked before committing queued writes or results. A verified identity replacement SHALL clear old owned/Family facts before new ownership can be applied.

#### Scenario: Late login after sign-out
- **WHEN** an invalidated login attempts to save its credential or publish success
- **THEN** neither operation restores the old account

#### Scenario: New account cannot load its catalog
- **WHEN** a verified different identity replaces the old account and catalog retrieval fails
- **THEN** old ownership stays cleared while installed games, matches, covers and history remain

### Requirement: Login windows are isolated and scoped
`auth.webview` SHALL authorize opening, focusing, cancelling and closing a bounded login attempt. The host SHALL enforce approved HTTPS navigation, an ephemeral profile and no frontend command access from remote content. Cancelling, closing or timing out SHALL preserve the previous account without saving an unsuccessful attempt.

#### Scenario: Steam Guard
- **WHEN** the user completes Steam login including an applicable Guard challenge
- **THEN** the isolated window supports the approved flow and the plugin validates the resulting session

#### Scenario: User closes during extraction
- **WHEN** the user closes the login window while cookie collection or page extraction is pending
- **THEN** the desktop remains alive, the attempt ends without a late successful login and temporary resources are cleaned up

#### Scenario: External link
- **WHEN** the login flow targets an external approved HTTPS link
- **THEN** it follows the declared browser policy without granting that page application access

### Requirement: Login extraction follows approved policy
`auth.webview.cookies.read` SHALL return only approved cookies from the caller's active attempt. `auth.webview.page.read` SHALL return only the approved bounded probe on its exact allowed origin. Secret-bearing responses SHALL remain outside logs, errors, snapshots and React state.

#### Scenario: Authenticated page supplies session context
- **WHEN** the approved cookie collection and page probe establish a Steam session
- **THEN** the plugin receives the identity and temporary token needed to validate it

#### Scenario: Undeclared origin or probe
- **WHEN** a request exceeds the trusted origin, cookie or probe policy
- **THEN** the host rejects it without executing arbitrary extraction

### Requirement: Secret permissions protect provider-owned credentials
`secrets.read`, `secrets.write` and `secrets.delete` SHALL be independently authorized for the caller's logical namespace. Windows SHALL use Credential Manager without a plaintext fallback. Linux SHALL retain Secret Service or its existing private-file fallback. Replies SHALL distinguish absence, success, locked/unavailable storage and failure.

#### Scenario: Linux fallback is used
- **WHEN** Secret Service is absent and the existing fallback stores a Steam credential
- **THEN** the file is private to the OS user and the GUI identifies unencrypted storage

#### Scenario: Windows storage fails
- **WHEN** Credential Manager cannot save a credential
- **THEN** failure is reported without writing a plaintext replacement

#### Scenario: Sign-out deletion fails
- **WHEN** the authoritative credential cannot be deleted
- **THEN** sign-out reports failure and preserves existing mainline account behavior

### Requirement: Existing Steam credentials remain authoritative
Upgrading SHALL preserve access to existing valid Steam credentials and account metadata without forcing login. Successful sign-out SHALL remove the authoritative credential, and legacy restore paths SHALL NOT resurrect it. Profile isolation used for validation SHALL NOT modify the user's live credentials.

#### Scenario: Mainline profile upgrades
- **WHEN** an existing profile with a valid Steam credential opens in the integrated application
- **THEN** the plugin restores it through the host and keeps its stable library identities

#### Scenario: Restart after sign-out
- **WHEN** the application restarts after successful sign-out
- **THEN** no legacy credential alias silently reconnects the account

### Requirement: Counter permission reports observations
`playtime.counter` SHALL accept validated provider-local totals and last-played observations. Core SHALL retain tracking, Store/Internal selection, GameNode attribution, deduplication and submission policy. Missing values SHALL leave prior totals unchanged and SHALL NOT introduce new measured-session submission behavior.

#### Scenario: Windows gameplay exits
- **WHEN** Core observes a Steam game exit with Store selected
- **THEN** it requests plugin counter observations and applies existing submission rules

#### Scenario: Preference changes during a request
- **WHEN** the user switches to Internal or changes GameNode account before completion
- **THEN** the old observation cannot submit totals under obsolete context

### Requirement: Action declarations remain host-validated
`provider.actions.declare` SHALL accept Play/Install declarations only within the trusted descriptor's schemes and templates. The host SHALL validate and execute the filled target without a shell. The grant SHALL NOT authorize arbitrary executables, filesystem installation or GameNode REST calls.

#### Scenario: Catalog-only Steam entry
- **WHEN** the user invokes the supported action for an owned or shared uninstalled game
- **THEN** the host uses the approved Steam URI declaration and existing launcher behavior

#### Scenario: Declaration widens authority
- **WHEN** a plugin declares a target outside its approved schemes and templates
- **THEN** the host rejects it without launching anything

### Requirement: Steam outputs do not grant GameNode authority
Plugins SHALL return public observations while Core/Engine retains GameNode authentication, reconciliation, matching, favorites, covers, tracking, persistence and synchronization. The API SHALL expose no GameNode credential, raw REST access or unrelated library mutation.

#### Scenario: Owned games arrive
- **WHEN** authorized Steam ownership results complete
- **THEN** Core reconciles them while preserving stable IDs and saved user metadata

#### Scenario: GameNode account changes
- **WHEN** provider results arrive for obsolete application-account context
- **THEN** they cannot apply or submit data under that obsolete context

### Requirement: Native process access is described accurately
Trusted metadata SHALL declare Steam's native file, HTTPS and Windows registry access separately from host-enforced service grants. A same-user native process SHALL NOT be presented as sandboxed, and API authorization SHALL remain required for host-owned resources.

#### Scenario: Steam reads its own configuration
- **WHEN** the plugin reads local Steam files or contacts Steam directly
- **THEN** documentation identifies native process access rather than claiming a host-enforced file or network sandbox
