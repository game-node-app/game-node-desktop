# Design

## Context

The plugin host advertises its contract through `DescribeApi`; the conformance script already
proves that every advertised route exists and that only two legacy methods stay unadvertised.
What is missing is evidence that a **real** consumer crosses each route with real grants, a real
profile and a real supervised process.

`docs/plugin-endpoint-acceptance.md` records the current state: provider discovery, custom launch,
runtime read/request, files read, `DataPublish`, storage and the control plane are crossed by
Dolphin; the provider account/login/catalog path by Steam and Epic; everything else is fixture-only.

## Goals / Non-Goals

**Goals**

- One reusable acceptance bundle that drives every cold endpoint with public API 1.6 calls.
- A report that maps probe id → endpoint → result → status code, so a failure names the endpoint.
- A maintained coverage page that fails review when a new route is unclassified.
- Prove the consent boundaries: grant, revoke in flight, profile change, uninstall/reinstall.

**Non-Goals**

- Replacing unit and fixture tests, or running in production installs.
- Driving native dialogs programmatically.
- New protocol methods, permissions or provider capabilities.
- Windows native acceptance; that remains a handoff.

## Decisions

### D1. One bundle, two components

Provider-only and extension-only permissions differ: a provider sees its own catalog, an extension
sees the host-selected game. A single component cannot cross both scopes without holding authority
it does not need. The bundle therefore ships one `provider` and one `extension`, installed and
enabled together, exactly like the Dolphin bundle that exposed the last set of bugs.

### D2. Development artifact inside the repository

The probe lives in `examples/plugins/rust-endpoint-probe` and is built with the other examples. It
uses the development publisher key, is never published, and its manifest name says it is a
development probe. Keeping it in-tree lets CI compile and test it and lets any contributor run the
matrix against their build.

### D3. Scenario in, report out

The probe reads a JSON scenario naming probe ids and expectations. Each probe is independent and
returns `passed`, `failed` or `skipped` with the observed status code and a short message. The
report is written three ways:

- a human-readable log on stdout;
- a persistent `DataPublish` dataset so the result survives the provider stopping;
- `StoragePut` for the last run summary.

Writing the report through the public API also exercises it. A probe that cannot write its report
is itself a finding.

### D4. Probe ids mirror matrix rows

Each probe id is stable and maps one-to-one to a row in
`docs/plugin-endpoint-acceptance.md`. A probe that covers several routes names them all. Renaming a
route or probe requires updating both sides in the same change.

### D5. Interactive consent is operator-driven

Native approvals are never automated. A probe that needs a folder, an origin, a runtime or a backup
scope emits a notification, states what to approve, and waits for confirmation in the Configure
window. It records the grant revision before and after, so a silent grant change is visible.

### D6. Conformance classifies every route

`scripts/check-plugin-conformance.py` gains a check that every advertised route is either present
in the acceptance page or listed in the documented deferral set. A route without a row fails the
script, which is the enforcement that keeps the page honest.

### D7. Authority discipline

Provider probes read only their own entries. Extension probes read only the host-selected game.
The probe never requests `library.read.all`, never supplies a path or executable, and never
self-grants. If a probe needs an endpoint that does not exist, the run records a finding instead of
working around the contract.

### D8. Results are evidence, not certification

A passing probe proves that the host path works with one real consumer. It does not certify native
code safety, OS-specific behavior or Windows execution. Those stay with native acceptance and the
Lamarck handoff.

## Risks / Trade-offs

- **The probe grows into a second product.** Mitigation: dev-only manifest, no distribution, one
  probe per matrix row, and no host-side changes unless a real defect is found.
- **Test-only permissions mask real consent.** Mitigation: the operator-driven steps and the
  recorded grant revisions; a probe that cannot show the revision is marked failed.
- **The matrix rots.** Mitigation: the conformance gate and the rule that a new route updates the
  page in the same change.
- **A probe passes while a user path still fails.** Mitigation: probes call the same public methods
  the SDK exposes, and custom-launch probes go through the real plan review.

## Migration Plan

1. Add the acceptance page and the conformance classification gate.
2. Create the bundle skeleton, manifests, packaging and CI build.
3. Implement provider probes, then extension probes.
4. Add the scenario/report path and the Configure panel.
5. Run the matrix on Linux, record results, hand off Windows acceptance.

## Open Questions

- Whether the report should also be written to an approved folder (needs a folder grant) or only
  through the public API. Current default: public API only, to avoid a second consent surface.
