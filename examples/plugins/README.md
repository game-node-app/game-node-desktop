# Third-party plugin examples

The Rust MVP provider demonstrates API 1.6 session negotiation, authenticated provider
context, fictional discovery, catalog records, and playtime counters. They do not implement game
actions, custom launch, file access, shared data, or telemetry.

## Build and fixture checks

Run from the repository root:

```sh
cargo test --manifest-path examples/plugins/rust-sdk/Cargo.toml
```

The Rust example uses the published `gamenode-plugin-sdk` dependency. Its local fixture can be run
before registry publication with a temporary Cargo patch to this checkout's matching SDK source.
Clients in other languages can generate bindings from the public
[`plugin.proto`](../../crates/plugin-protocol/proto/plugin.proto) and follow the same session and
permission rules; the repository does not ship a non-Rust example.

The [Rust configuration example](rust-configuration/README.md) shows plugin-owned configuration and
persistent preference storage. Those features are outside the minimal Rust provider example
contract.

## Package and sign

The MVP example has a self-contained `package/` directory with its `plugin.json`, MIT license and
third-party notices. Its README shows how to build the binary, generate a development-only key,
replace the placeholder publisher key, and run `scripts/plugin-package.py pack` and `verify`.
Development keys exist for local import fixtures; they are not production identities, and the
private key must never be committed.

The packaged example is exercised by the host's production import review, publisher
confirmation, signature/integrity validation, license display and permission review in
`plugin_host::updates::tests::mvp_example_packages_use_production_import_and_consent_validators`.
That test signs fixtures derived from these manifests; it does not execute the examples.

A product that ships a provider and an extension together can package both `.gnplugin` components
into one signed `.gnbundle` with `scripts/plugin-package.py bundle` and verify it with
`bundle-verify`. See [bundling a provider and an extension](../../docs/plugin-development.md#bundling-a-provider-and-an-extension)
and the [bundle format](../../docs/plugin-package-manifest.md#plugin-bundles) for the commands,
limits and import behavior.

Run `python3 scripts/check-plugin-conformance.py` from the repository root after touching the
protocol, an example manifest, or the served-method tables. See
[`docs/plugin-development.md`](../../docs/plugin-development.md#conformance-checks-for-examples-and-the-public-surface).
