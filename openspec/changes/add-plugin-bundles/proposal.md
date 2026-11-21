# Proposal

## Why

Some third-party capabilities cannot live in one plugin package: GameNode registers a package as
either a provider or an extension, and each kind has its own session, process lifecycle and
consent. A real plugin such as the Dolphin emulator integration therefore needs two packages — a
provider that discovers GameCube/Wii games and an extension that starts them — but asking the user
to import two files, confirm two publisher fingerprints, see two inventory rows and uninstall
twice is poor UX and weakens the trust story: the user cannot tell that the two halves are one
product.

A signed bundle gives the user one installable, one review and one inventory entry while the host
keeps the two component identities, processes and grants that the security model requires.

## What Changes

- Add a signed bundle envelope (`.gnbundle`, ZIP) containing a `bundle.json` and one or more
  existing `.gnplugin` component packages under `components/`.
- `bundle.json` declares the bundle id, name, SemVer version, publisher key and, for each
  component, its package-relative path, SHA-256, plugin id and kind.
- The host verifies the bundle signature over the canonical `bundle.json` bytes, every component
  hash, and each component's existing package signature. All components must share the bundle
  publisher key. Nested bundles are rejected.
- Structural failure (missing component, hash mismatch, traversal, duplicate plugin id, publisher
  mismatch, unsupported schema) rejects the whole bundle atomically. A component that is only
  platform- or API-incompatible is installed unavailable and does not block its siblings; this is
  what lets the discovery half work while the launch half waits for extension support.
- Import shows one review and pins one publisher fingerprint. Permissions are grouped by
  component and remain granted per component identity; approving the bundle grants nothing.
- The inventory shows one row per bundle with the bundle name/version/publisher and each
  component's status. Enable/disable remains per component in the details. One uninstall stops
  and removes every component and applies one persistence-retention choice.
- Bundle update and rollback are atomic: all components swap together or none do. The review
  shows each component's old/new version and permission diff; component versions and API ranges
  stay independent inside the bundle version.
- Add bundle packing/verification to the repository tooling, with a development-key marker, and
  document the format, review, uninstall and rollback behavior.
- The existing single-package import, consent, update and uninstall paths remain unchanged. This
  is additive; there is no **BREAKING** change. A single manifest with multiple `kind` values and
  a single process serving both kinds remain out of scope.

## Capabilities

### New Capabilities

- `desktop-plugin-bundles`: signed multi-package bundles, their verification, grouped consent,
  grouped inventory, cascade uninstall, atomic update/rollback and tooling.

### Modified Capabilities

- `desktop-external-plugins`: package import, consent and lifecycle accept a bundle envelope while
  preserving per-component identity, signatures, permissions and failure isolation.
- `desktop-settings`: the Stores plugin inventory groups bundle components under one entry with
  one review and one uninstall while reporting each component's status, without implying a
  sandbox.

## Impact

- Host: bundle parser/verifier, import staging, registry bundle-to-component mapping (migration),
  atomic install/activation/rollback, uninstall cascade, inventory aggregation and Tauri commands.
- UI: one grouped plugin row with component statuses, grouped permission review, grouped uninstall
  and bundle update/rollback views, en/pt-BR labels and mocks.
- Tooling and docs: `scripts/plugin-package.py` bundle command (or a sibling tool), signature
  domain, package-manifest/registry/update documentation.
- Tests: bundle signature/hash/tamper, publisher mismatch, duplicate ids, missing component,
  platform-incompatible component, atomic failure, grouped review, inventory grouping, uninstall
  cascade, update/rollback and nested-archive limits.
- First consumer: `richmont/gamenode-plugin-dolphin-emu`, which already builds a discovery
  provider and a launch extension and can ship both as one bundle. Component identity and
  permissions do not change.
- Out of scope: a combined provider/extension package kind, a single process serving both
  session kinds, marketplace/publishing infrastructure, and changing the per-component consent
  semantics.
