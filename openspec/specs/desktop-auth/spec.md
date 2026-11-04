# desktop-auth Specification

## Purpose

Authenticate GameNode Desktop users through the existing GameNode browser flow while keeping credentials out of ordinary local application data and rejecting unsolicited callbacks.

## Requirements

### Requirement: Sign-in through the system browser
The application SHALL start sign-in using the existing GameNode mobile authentication contract and open the system browser. It SHALL accept success or failure through `app.gamenode://` without rendering web content in the application.

#### Scenario: Successful sign-in
- **WHEN** the user starts sign-in and completes an existing GameNode provider flow in the browser
- **THEN** the matching callback authenticates the application, persists its session credential, and shows the signed-in user above the empty library

#### Scenario: Sign-in cannot complete
- **WHEN** opening the browser fails, the provider rejects sign-in, the attempt expires, or the user cancels
- **THEN** the application remains signed out, presents a localized status, and permits another sign-in attempt

### Requirement: Bind callbacks to the initiated attempt
The application SHALL accept authentication results only for its current, unexpired sign-in attempt and exact expected callback target. It SHALL reject malformed, unsolicited, mismatched, expired, and replayed callbacks without replacing the active session. Credentials and complete callback URLs SHALL NOT appear in logs or errors.

#### Scenario: Callback from the initiated browser flow
- **WHEN** a callback has the expected scheme, target, and correlation value for the active attempt
- **THEN** the application validates the returned session with GameNode before accepting the identity
- **AND** that attempt cannot be consumed a second time

#### Scenario: Callback is unsolicited or replayed
- **WHEN** a callback has no active attempt, a different target or correlation value, expired correlation, invalid credential data, or a previously consumed attempt
- **THEN** it does not authenticate the application or change its session
- **AND** any diagnostic excludes the callback credential

### Requirement: Protected session persistence
The application SHALL persist the session token in Windows Credential Manager on Windows or Secret Service on Linux. Only when Linux has no Secret Service SHALL it use a credential file accessible only to the current user. Tokens SHALL NOT be written to the ordinary settings database.

#### Scenario: Keyring is available
- **WHEN** sign-in succeeds with the OS credential service available
- **THEN** the credential is stored in that service and Settings identifies keyring storage

#### Scenario: Linux has no Secret Service
- **WHEN** sign-in succeeds on Linux without a Secret Service implementation
- **THEN** the token is stored in a private file with user-only permissions and Settings discloses the fallback

#### Scenario: Storage is locked or persistence fails
- **WHEN** the configured credential service exists but is locked or writing the credential fails
- **THEN** the application reports the persistence failure without silently switching to an unprotected store or claiming that restart restoration will work

### Requirement: Restore and validate the session
On startup the application SHALL restore a persisted session and obtain the signed-in identity from GameNode. An expired or revoked session SHALL return to sign-in. Temporary network failure SHALL preserve the stored credential and allow retry without treating it as rejected. While retrying, the application SHALL show the cached library of the last signed-in account, with network actions disabled until verification succeeds.

#### Scenario: Restart signed in
- **WHEN** the application restarts with a valid stored credential and GameNode is reachable
- **THEN** it shows the signed-in user and their library without another browser sign-in

#### Scenario: Session is rejected
- **WHEN** GameNode rejects the restored credential as invalid or expired
- **THEN** the application clears the rejected local credential, hides the cached library, and offers sign-in

#### Scenario: Network unavailable during restoration
- **WHEN** restoring a session fails because GameNode is temporarily unreachable
- **THEN** the application retains the credential, shows a recoverable connection status, and offers retry
- **AND** it shows the last signed-in account's cached library, where Play and Install work and matching and Add to GameNode are disabled
- **AND** Settings/About remains accessible

#### Scenario: Verification succeeds after an offline start
- **WHEN** a retry verifies the session for the same account
- **THEN** the library stays open and network actions become available

#### Scenario: Verified account differs from the cached one
- **WHEN** the verified session belongs to a different account than the cached library
- **THEN** the cached library of the previous account is replaced by the verified account's library

### Requirement: Sign-out clears local authentication
Sign-out SHALL clear persisted credentials, in-memory identity, and pending sign-in attempts and return the shell to signed out. The application SHALL also attempt to invalidate the current server session. Server unavailability SHALL NOT prevent local sign-out, and unsuccessful credential deletion SHALL be reported.

#### Scenario: Sign-out while online
- **WHEN** the signed-in user signs out with GameNode reachable
- **THEN** the current server session is invalidated and local authentication is removed
- **AND** restarting the application does not restore that session

#### Scenario: Sign-out while offline
- **WHEN** the user signs out while GameNode is unreachable
- **THEN** local authentication is removed and the application explains that server invalidation could not be confirmed
- **AND** a late callback from a prior attempt cannot sign the user back in
