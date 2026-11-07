# Spec Delta

## Purpose

Connect store accounts through an isolated web sign-in, protect and refresh the resulting
credential, and expose a clear connection status without affecting installed games.

## ADDED Requirements

### Requirement: Store account connection is opt-in
The application SHALL offer a store account connection only for a loaded provider that declares a
catalog. It SHALL NOT start a sign-in automatically, and an account SHALL NOT be required to list
installed games.

#### Scenario: No account connected
- **WHEN** a provider with a catalog has no stored credential
- **THEN** its account is shown as not connected and sign-in is offered

#### Scenario: Provider without a catalog
- **WHEN** a loaded provider declares no catalog
- **THEN** no account control or sign-in action is shown for it

#### Scenario: Installed games without an account
- **WHEN** no store account is connected
- **THEN** the provider still lists the games installed on this PC

### Requirement: Isolated web sign-in window
The application SHALL perform store sign-in in a separate application window that loads the
store's own HTTPS login page. The main window SHALL NOT render store content. The sign-in window
SHALL have no access to application commands, SHALL use a dedicated ephemeral profile, and SHALL
allow navigation only within the store's sign-in flow.

#### Scenario: Successful sign-in
- **WHEN** the user starts Steam sign-in and completes Steam's login, including any Steam Guard step
- **THEN** the window is closed, the session is validated, and the account becomes connected

#### Scenario: User cancels or closes
- **WHEN** the user closes the sign-in window, chooses Cancel, or the attempt expires
- **THEN** no credential is saved and the previous account state is unchanged

#### Scenario: External link
- **WHEN** the sign-in page navigates to a non-Steam HTTPS link
- **THEN** the link opens in the system browser and the sign-in window stays on Steam

### Requirement: Protected session credential
On a successful sign-in the application SHALL read the store session cookie from the sign-in
window and store it in a credential slot separate from the GameNode session. On Windows it SHALL
use Credential Manager; on Linux it SHALL use Secret Service or, when no Secret Service exists, a
private file readable only by the current user. The credential SHALL NOT be written to the
settings database, logs, errors, or snapshots.

#### Scenario: OS keyring available
- **WHEN** sign-in succeeds and the OS credential service is available
- **THEN** the cookie is stored there and Stores identifies keyring storage

#### Scenario: Linux without Secret Service
- **WHEN** sign-in succeeds on Linux with no Secret Service
- **THEN** the cookie is stored in a user-only private file and Stores warns that it is not encrypted

#### Scenario: Credential cannot be saved
- **WHEN** storing the credential fails or the service is locked
- **THEN** the connection works for this run only, Stores reports that it will not be restored, and no plaintext fallback is written on Windows

### Requirement: Automatic session refresh
On startup and before each catalog sync the application SHALL validate the stored cookie against
the store's authenticated page. It SHALL derive the account identity and a short-lived API token
from that page, update the stored cookie if the store rotates it, and SHALL NOT persist the
short-lived token.

#### Scenario: Restart with a valid session
- **WHEN** the application starts with a stored cookie that Steam still accepts
- **THEN** the account becomes connected without another sign-in and the catalog sync starts

#### Scenario: Rotated cookie
- **WHEN** the store returns a new session cookie
- **THEN** the new value replaces the stored one before the sync completes

#### Scenario: Expired or revoked session
- **WHEN** Steam reports the session as logged out or rejects the API token
- **THEN** the account becomes expired, the dead credential is removed, and the cached library remains visible

#### Scenario: Steam unreachable
- **WHEN** the store page cannot be reached or returns a server error
- **THEN** the credential is kept, the account becomes unreachable, and the cached library remains visible

### Requirement: Account status and actions
The application SHALL expose one connection state per catalog provider: not connected, checking,
signing in, connected, expired, unreachable, or credential error. It SHALL show the account
identity, the last successful check, the available actions, and the credential storage method.
State SHALL be conveyed by text and icon, not color alone.

#### Scenario: Connected
- **WHEN** the account is connected
- **THEN** Stores shows the account name, last check, storage method, and offers Sync now and Sign out

#### Scenario: Expired
- **WHEN** the session has expired
- **THEN** Stores explains that installed games still work and offers Sign in again and Sign out

#### Scenario: Unreachable
- **WHEN** Steam cannot be reached
- **THEN** Stores explains that the saved library is shown and offers Try again

#### Scenario: Credential error
- **WHEN** the saved credential cannot be read or the keyring is locked
- **THEN** Stores offers Retry and explains how to unlock or replace the credential

### Requirement: Status checks are fresh and non-blocking
The application SHALL check the stored session when Stores is opened if the last check is older
than a short interval, and when the user asks. Checks and catalog syncs SHALL run off the UI
thread, SHALL coalesce while one is running, and SHALL NOT block other settings topics or the
library.

#### Scenario: Stores opened after a while
- **WHEN** the user opens Stores and the last check is older than the interval
- **THEN** a check starts and the card shows progress until it finishes

#### Scenario: Stores opened immediately after a check
- **WHEN** the user opens Stores within the interval
- **THEN** the cached status is shown without another request

#### Scenario: Repeated refresh
- **WHEN** the user triggers refresh while a check is running
- **THEN** no second request starts and the running result is used

### Requirement: Sign-out clears the store account
Sign-out SHALL delete the stored credential, clear the in-memory session, and clear the account's
owned and family-shared facts from the library while keeping installed games, matches, and covers.
A failed deletion SHALL be reported, and the application SHALL NOT claim that a restart will stay
signed out.

#### Scenario: Sign-out succeeds
- **WHEN** the user signs out of Steam
- **THEN** the account is not connected, owned and family-shared games leave the library, and installed Steam games remain

#### Scenario: Sign-out deletion fails
- **WHEN** the credential cannot be deleted
- **THEN** the account remains connected for this run and Stores reports the failure

#### Scenario: Late sign-in result
- **WHEN** a sign-in window from an earlier attempt reports success after sign-out
- **THEN** it does not reconnect the account

### Requirement: Account changes replace the catalog
When a refresh identifies a different store account than the stored one, the application SHALL
replace the stored credential and clear the previous account's owned and family-shared facts before
applying the new catalog. A failure after the identity change SHALL NOT leave the previous
account's games visible as owned.

#### Scenario: Switch accounts
- **WHEN** the user signs in with a different Steam account
- **THEN** the previous account's owned and family-shared games are cleared and the new account's catalog is applied

#### Scenario: New account cannot load its catalog
- **WHEN** the account changes but the catalog request then fails
- **THEN** the previous account's owned and family-shared games are not shown

### Requirement: Credential failures are secret-free
Errors, logs, and snapshots SHALL identify the failure category without including the cookie, the
short-lived token, the full store response, or the complete account URL.

#### Scenario: Store page cannot be parsed
- **WHEN** the authenticated store page does not contain the expected data
- **THEN** the account shows a localized "Steam changed its response" error and the log contains no token or cookie

#### Scenario: Network failure
- **WHEN** a Steam request fails
- **THEN** the error identifies a connection problem and no request headers are logged
