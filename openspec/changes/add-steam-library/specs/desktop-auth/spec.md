# Spec Delta

## MODIFIED Requirements

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
