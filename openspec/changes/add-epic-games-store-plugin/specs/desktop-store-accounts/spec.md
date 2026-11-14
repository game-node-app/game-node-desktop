# Spec Delta

## MODIFIED Requirements

### Requirement: Isolated web sign-in window
The application SHALL perform store sign-in in a separate application window that loads the
store's own HTTPS login page. The main window SHALL NOT render store content. The sign-in window
SHALL have no access to application commands. It SHALL use a dedicated ephemeral profile for that
provider and SHALL allow navigation only within the store's sign-in flow, as approved by the
provider's trusted login policy.

#### Scenario: Successful sign-in
- **WHEN** the user starts sign-in for a store and completes its login, including any second factor such as Steam Guard or Epic two-factor authentication
- **THEN** the window is closed, the session is validated by that store's plugin, and the account becomes connected

#### Scenario: User cancels or closes
- **WHEN** the user closes the sign-in window, chooses Cancel, or the attempt expires
- **THEN** no credential is saved and the previous account state is unchanged

#### Scenario: External link
- **WHEN** the sign-in page navigates to an HTTPS link outside the provider's approved hosts
- **THEN** the link opens in the system browser and the sign-in window stays on the store

#### Scenario: Profiles are separate per store
- **WHEN** the user signs in to Steam and later to Epic
- **THEN** each attempt uses its own ephemeral profile, and neither store's cookies or storage are visible to the other

### Requirement: Protected session credential
On a successful sign-in the store's plugin SHALL store its long-lived credential through the host
in that provider's own credential slot, separate from the GameNode session and from every other
provider. Examples are Steam's session cookie and Epic's refresh token. On Windows the slot SHALL
use Credential Manager. On Linux it SHALL use Secret Service or, when no Secret Service exists, a
private file readable only by the current user. The credential SHALL NOT be written to the
settings database, logs, errors, or snapshots.

#### Scenario: OS keyring available
- **WHEN** sign-in succeeds and the OS credential service is available
- **THEN** the credential is stored there and Stores identifies keyring storage

#### Scenario: Linux without Secret Service
- **WHEN** sign-in succeeds on Linux with no Secret Service
- **THEN** the credential is stored in a user-only private file and Stores warns that it is not encrypted

#### Scenario: Credential cannot be saved
- **WHEN** storing the credential fails or the service is locked
- **THEN** the connection works for this run only, Stores reports that it will not be restored, and no plaintext fallback is written on Windows

#### Scenario: Existing Steam credential
- **WHEN** a profile that already has a stored Steam session is opened after this change
- **THEN** the same physical Steam credential is used without migration, renaming or a new sign-in

### Requirement: Automatic session refresh
On startup and before each catalog sync, the store's plugin SHALL validate its stored credential
with the store. It SHALL derive the account identity and any short-lived access token from that
validation, and SHALL replace the stored credential when the store rotates it. Short-lived tokens
SHALL NOT be persisted.

#### Scenario: Restart with a valid session
- **WHEN** the application starts with a stored credential that the store still accepts
- **THEN** the account becomes connected without another sign-in and the catalog sync starts

#### Scenario: Rotated cookie
- **WHEN** the store returns a new session cookie or refresh token
- **THEN** the new value replaces the stored one before the sync completes

#### Scenario: Expired or revoked session
- **WHEN** the store reports the session as logged out or rejects the credential
- **THEN** the account becomes expired, the dead credential is removed, and the cached library remains visible

#### Scenario: Steam unreachable
- **WHEN** the store cannot be reached or returns a server error
- **THEN** the credential is kept, the account becomes unreachable, and the cached library remains visible

### Requirement: Account status and actions
The application SHALL expose one connection state per catalog provider: not connected, checking,
signing in, connected, expired, unreachable, or credential error. It SHALL show the account
identity, the last successful check, the available actions, and the credential storage method.
State SHALL be conveyed by text and icon, not color alone. The same account controls SHALL be
rendered for every catalog provider and labeled with that provider's display name.

#### Scenario: Connected
- **WHEN** the account is connected
- **THEN** Stores shows the account name, last check, storage method, and offers Sync now and Sign out

#### Scenario: Expired
- **WHEN** the session has expired
- **THEN** Stores explains that installed games still work and offers Sign in again and Sign out

#### Scenario: Unreachable
- **WHEN** the store cannot be reached
- **THEN** Stores explains that the saved library is shown and offers Try again

#### Scenario: Credential error
- **WHEN** the saved credential cannot be read or the keyring is locked
- **THEN** Stores offers Retry and explains how to unlock or replace the credential

#### Scenario: Two stores connected
- **WHEN** Steam and Epic both declare a catalog
- **THEN** Stores shows one account card for each, and actions on one card never affect the other provider's account

### Requirement: Credential failures are secret-free
Errors, logs, and snapshots SHALL identify the failure category without including any credential,
authorization code, short-lived token, full store response, or complete account URL. Categories
SHALL be provider-neutral and SHALL be shown with the provider's display name.

#### Scenario: Store page cannot be parsed
- **WHEN** a store's authenticated response does not contain the expected data
- **THEN** the account shows a localized "<store> changed its response" error, and the log contains no token, cookie or code

#### Scenario: Network failure
- **WHEN** a store request fails
- **THEN** the error identifies a connection problem and no request headers are logged
