# Spec Delta

## ADDED Requirements

### Requirement: Runtime access uses a host picker

An extension SHALL request an approved runtime through `RuntimesRequestAccess` under the existing
`game.runtime.create` permission. The request SHALL carry the runtime spec without an executable
handle plus text executable hints. The host SHALL show a picker, validate the selected regular
executable and create the runtime for the caller. The reply SHALL contain the runtime record
without an executable path or file handle.

#### Scenario: First runtime request

- **WHEN** an enabled extension requests a runtime that it does not own yet
- **THEN** the host shows the picker, the user approves one executable, and the reply carries the
  created runtime record without a path or handle

#### Scenario: Picker declined

- **WHEN** the user cancels or declines the runtime picker
- **THEN** no runtime or approval is created and the extension receives a safe refusal

### Requirement: Runtime approvals are durable and revalidated

The host SHALL persist the approved executable identity and digest, SHALL return the same runtime
for an idempotent retry, and SHALL require fresh approval when the executable changes. A changed
approval SHALL NOT fall back to another path or launch silently.

#### Scenario: Retry with the same key

- **WHEN** the extension retries the same request with the same idempotency key
- **THEN** it receives the same runtime and no second picker opens

#### Scenario: Changed executable

- **WHEN** the approved executable identity or digest no longer matches
- **THEN** the runtime cannot launch and the next request requires a fresh user approval
