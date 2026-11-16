# Spec Delta

## ADDED Requirements

### Requirement: Reopen the browser for a pending sign-in
While a sign-in started from this application is waiting for the browser, the application SHALL show that it is waiting and SHALL offer opening the browser again and cancelling. Opening again SHALL open the same attempt's address, so a callback from either browser tab completes the same attempt. It SHALL NOT start a new attempt or extend the attempt's expiry.

#### Scenario: Browser tab was closed
- **WHEN** the user closed the sign-in tab and chooses Open again
- **THEN** the default browser opens the same sign-in address and completing it signs the user in

#### Scenario: Reopen fails
- **WHEN** opening the browser again fails
- **THEN** a localized error is shown and the attempt keeps waiting until it completes, is cancelled or expires

#### Scenario: Cancel while waiting
- **WHEN** the user chooses Cancel sign-in while waiting
- **THEN** the attempt ends, a later callback for it is ignored, and the provider buttons are available again
