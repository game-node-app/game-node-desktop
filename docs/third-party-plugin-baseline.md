# Official plugin compatibility baseline

This record captures the delivered official-plugin contract before third-party
package paths are added. Native acceptance that is still tracked by
`integrate-steam-plugin-into-main` remains separate and is not implied by these
fixtures.

## Consolidated specifications

The surviving product contracts are maintained in:

- `openspec/specs/desktop-plugins/spec.md`
- `openspec/specs/desktop-plugin-services/spec.md`
- `openspec/specs/desktop-providers/spec.md`

The Steam migration's pending native acceptance is tracked in
`openspec/changes/integrate-steam-plugin-into-main/tasks.md` and
`docs/steam-plugin-integration.md`.

## Delivered descriptors and grants

The host-owned descriptors are the static `steam` and `packagekit` entries in
`crates/app/src/plugin_host.rs`. Provider backend mappings remain validated by
`crates/core/src/provider/registry.rs`. The descriptor, session, permission,
credential-slot and process behavior is covered by the focused tests in
`crates/app/src/plugin_host/`, `crates/plugin-protocol/tests/contract.rs`, and
`crates/plugin-sdk/tests/`.

These fixtures preserve the official API 1.1–1.3 negotiation behavior, the
known capability grants and operation gates, provider identity checks, and
credential separation. Third-party package identity or signature must not
change official provider identity, credentials, or grants.

## Packaging and executable boundary

Official plugin packaging and bundle behavior are recorded in
`docs/plugin-builds.md`, `scripts/package-linux.sh`, and
`scripts/package-windows-wsl.sh`. Package verification fixtures live in
`scripts/tests/test_plugin_package.py`; boundary checks live in
`scripts/tests/test_plugin_boundaries.py`. Third-party packages are additive
and do not alter the official descriptors or packaging inputs.

## Regression commands

Run the focused delivered-baseline fixtures with:

```sh
cargo test --locked -p gamenode-plugin-protocol
cargo test --locked -p gamenode-plugin-sdk
cargo test --locked -p gamenode-app plugin_host
python3 -m unittest scripts.tests.test_plugin_package scripts.tests.test_plugin_boundaries
```

Verified on 2026-11-10: protocol contract/settings/extension fixtures passed
(12 + 7 + 3), SDK reference/settings fixtures passed (6 + 5), official host
plugin tests passed (49), and package/boundary Python fixtures passed (11).
These tests exercise the delivered compatibility behavior alongside additive
API coverage.

Passing these checks records the delivered software baseline only. It does not
claim the separately pending native Steam acceptance checks have passed.
