# Proposal

## Why

The public plugin contract advertises 85 routes. Roughly a third has ever been crossed by a real
signed bundle; the rest is covered only by Rust fixtures that construct the happy state directly.
The Dolphin integration showed the cost of that gap: seven host bugs appeared only when a real
provider and a real extension ran under the host — revision bookkeeping, an absent detected target,
desktop session inheritance, live lifecycle without a restart and a store-shaped launch UI. All
fixes landed on shared host paths, so the remaining cold paths will find the same classes of bug
one plugin at a time, late.

Writing one plugin per capability is slow and each new plugin repeats the same packaging, signing
and consent work. A single acceptance bundle that drives every endpoint is more productive.

## What Changes

- **Coverage record.** `docs/plugin-endpoint-acceptance.md` becomes the source of truth for endpoint
  homologation: one row per family, status (`third-party`, `official`, `example`, `fixture`,
  `none`), consumer and evidence, plus the procedure (grant → use → revoke → profile change →
  reinstall).
- **Development endpoint probe.** A new signed bundle under `examples/plugins/rust-endpoint-probe`
  with two components: a **provider** (control plane, discovery, catalog, account, storage, data
  publish, files read) and an **extension** (selected-game launch, runtime, tools, ports, backups,
  notifications, telemetry, configuration).
- **Scenario and report.** The probe reads a JSON scenario listing probe ids and expectations and
  writes a machine-readable report through the public API (`DataPublish` persistent and
  `StoragePut`), plus a human log. Each probe reports `passed`, `failed` or `skipped` with the
  exact status code; nothing is swallowed.
- **Interactive steps stay real.** Folder, origin, runtime and backup approvals remain host dialogs
  driven by the operator; the probe emits a notification, waits for the Configure-window
  confirmation and records the grant revision and timestamp.
- **Conformance gate.** `scripts/check-plugin-conformance.py` fails when an advertised route has no
  matrix row and no documented deferral, so a new endpoint cannot ship unclassified.
- **Homologation run.** Execute the matrix on Linux, record results on the page, and keep native
  Windows acceptance as an open task.

## Capabilities

### Modified Capabilities

- `desktop-plugin-development`: endpoint homologation requires a real consumer and a maintained
  probe bundle.

## Impact

- New crate and manifests under `examples/plugins/rust-endpoint-probe`, built and tested with the
  existing example scripts and the dev signing key.
- `docs/plugin-endpoint-acceptance.md`, `docs/plugin-development.md` link, and the conformance
  script's route classification.
- No production code path changes. No new protocol surface, permission, provider or store.
- The probe is a development artifact: dev publisher key, never distributed, never installed by an
  end user.

## Out of Scope

- Replacing fixtures: fixtures stay as regression tests.
- Automating native dialogs: the operator still approves folder, origin, runtime and backup
  scopes.
- Native Windows acceptance, which stays an open task until a native run is recorded.
- A marketplace or a shipped diagnostics plugin.
