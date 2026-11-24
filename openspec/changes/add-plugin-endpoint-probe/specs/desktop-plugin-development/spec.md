# Spec Delta

## ADDED Requirements

### Requirement: Endpoint homologation requires a real consumer
An endpoint in the public plugin contract SHALL be considered homologated only after a signed
bundle has crossed it under the host with real grants, the active account profile and a supervised
process. Fixture-only or example-only coverage SHALL be recorded as a gap, not as completion.

#### Scenario: New route ships unclassified
- **WHEN** a change adds or renames an advertised plugin route
- **THEN** the conformance check fails until the acceptance page classifies the route or documents
  why it is deferred

#### Scenario: Fixture passes but no bundle exists
- **WHEN** a route is covered only by a host fixture
- **THEN** its acceptance row stays at fixture status and the endpoint is not reported as
  homologated

#### Scenario: Grant is revoked in flight
- **WHEN** the operator revokes the grant a probe is using
- **THEN** the probe records a denial with its status code, no partial result is reported as
  success, and the acceptance row records the revocation evidence

### Requirement: Development endpoint probe bundle
The project SHALL provide a development-only probe bundle with a provider and an extension that
exercise the public API through the same methods an author uses. The probe SHALL NOT request
authority it does not need, SHALL NOT be distributed, and SHALL report per-probe results without
swallowing status codes.

#### Scenario: Run a probe scenario
- **WHEN** the operator starts the probe with a scenario
- **THEN** each probe reports passed, failed or skipped with its endpoint and status code, and the
  report is written through the public API

#### Scenario: Probe needs interactive consent
- **WHEN** a probe requires a folder, origin, runtime or backup approval
- **THEN** it states what must be approved, waits for the operator, and records the grant revision
  before and after

#### Scenario: Probe attempts to widen authority
- **WHEN** a probe asks for an endpoint or scope beyond its declared permissions
- **THEN** the host denies it and the probe records the denial as the probe result rather than
  retrying another path
