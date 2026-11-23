# Rust MVP provider

This standalone MIT example uses the public `gamenode-plugin-sdk` crate. It negotiates API 1.6,
opens an authenticated provider session, reads its host-issued provider context, and handles
fictional discovery, catalog, and playtime-counter operations. It requests only
`provider.context.read`, `library.discovery`, `library.catalog`, and `playtime.counter`.

The example does not implement launch extensions, file access, shared-data exchange, or telemetry.
The fake catalog and discovery records are fixtures; the executable does not scan or launch games.

## Build and run

After `gamenode-plugin-sdk@0.1.0` is published, run from the repository root:

```sh
cargo build --manifest-path examples/plugins/rust-sdk/Cargo.toml
```

GameNode starts the binary with its authenticated endpoint and session credential in the SDK's
documented environment variables. Before the SDK is published, run the fixture against this
checkout's SDK source with a temporary Cargo patch:

```sh
cargo test --manifest-path examples/plugins/rust-sdk/Cargo.toml --test provider_exchange \
  --config 'patch.crates-io.gamenode-plugin-sdk.path="crates/plugin-sdk"'
```

The patch is a command-line validation override; the example manifest continues to depend on the
exact published SDK version. The committed `Cargo.lock` was generated with the same patch, so it
records the SDK as a path package. Regenerate it once `gamenode-plugin-sdk@0.1.0` is published.

## Fixture host

The integration test starts an independent local gRPC host. It checks the API 1.6 range and exact
capability request, authenticates the session credential, returns provider context, and drives
Initialize, Discover, SyncCatalog, and ReadCounter through the SDK exchange loop. The fixture
asserts the emitted records and counter values.

## Package, sign and install

`package/` is the pack source: the manifest, the MIT license, and this example's third-party
notices. It declares one Linux x86-64 target; the host selects the entrypoint from the signed
target tuple.

1. Build a release binary and copy it to the declared entrypoint:

   ```sh
   cargo build --release --manifest-path examples/plugins/rust-sdk/Cargo.toml
   install -Dm755 \
     examples/plugins/rust-sdk/target/release/gamenode-plugin-mvp-example \
     examples/plugins/rust-sdk/package/bin/linux-x64/plugin
   ```

   To package Windows instead, change the `targets` entry in `package/plugin.json` to
   `{"os":"windows","arch":"x86_64","abi":"msvc","entrypoint":"bin/windows-x64/plugin.exe"}`,
   build with `--target x86_64-pc-windows-msvc`, and copy the `.exe` to that path. A target whose
   entrypoint is missing from the signed payload is rejected at import.

2. Generate a development-only signing key and note the printed public key:

   ```sh
   python3 -m pip install -r scripts/plugin-package-requirements.txt
   python3 scripts/plugin-package.py keygen --development --private-key ./fixture-dev.pem
   ```

   Replace `publisher.public_key` in `package/plugin.json` with the printed value and choose any
   `key_id`. The committed placeholder key cannot sign a package: `pack` refuses a private key that
   does not match the manifest publisher key. A development key adds `DEVELOPMENT-KEY.txt` to the
   archive and is never a production identity; keep the private key out of the repository.

3. Pack and verify without executing anything:

   ```sh
   python3 scripts/plugin-package.py pack \
     --source examples/plugins/rust-sdk/package \
     --private-key ./fixture-dev.pem \
     --output ./rust-mvp.gnplugin
   python3 scripts/plugin-package.py verify ./rust-mvp.gnplugin
   ```

4. In GameNode, import `rust-mvp.gnplugin`. The review shows the untrusted publisher fingerprint,
   the MIT license and these notices; confirm that exact fingerprint to pin the publisher, then
   install the package. It stays disabled until you review permissions: grant
   `provider.context.read` and enable the plugin. `library.discovery`, `library.catalog` and
   `playtime.counter` are optional and can stay denied. No launch-extension, file, shared-data or
   telemetry permission is requested.

5. After enabling, the fictional `example-rust` provider reports its fixture games. Account and
   catalog operations only succeed while a provider session is authenticated by the host; the
   example never starts a game.
