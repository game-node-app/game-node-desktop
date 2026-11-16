# Plugin Rust source releases

The public Rust crates are `gamenode-plugin-protocol` and
`gamenode-plugin-sdk`. Their Cargo package versions identify source releases and
are independent of the wire API version. Keep the SDK's exact protocol crate
version requirement synchronized with the protocol package version.

## Prepare a release

From the repository root, run the release packager with the required external
Cargo target:

```sh
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  python3 scripts/package-plugin-rust-sources.py --output dist/plugin-rust
```

The script requires clean public crate sources by default, checks Cargo's
effective target directory, asks Cargo to create the versioned `.crate`
packages, then extracts them into clean, temporary source
workspaces. It locks dependencies offline, checks that package metadata has no
path dependency back into the GameNode repository, and runs protocol tests plus
SDK library and integration tests from those extracted sources. All Cargo
build output stays in `CARGO_TARGET_DIR`; temporary source trees are removed
after verification.

The output directory contains the registry-format `.crate` files and
versioned `.tar.gz` source bundles. Each bundle includes its `Cargo.lock`, the
pinned `rust-toolchain.toml`, package source and a generated third-party
license inventory. It copies license text files present in the resolved crate
sources and explicitly lists packages whose source archive does not provide a
license text file. The SDK source bundle includes the matching protocol
source as a sibling crate so it can build before either crate is published.
Its SDK `Cargo.toml` still declares `gamenode-plugin-protocol` by exact version;
the bundle root patches that registry package to the copy contained in the
bundle. No dependency points to a path in the GameNode checkout. `SHA256SUMS`
records hashes for the generated package and source archives.

Review the versioned package manifests, lockfiles, dependency inventory,
included license texts and archive checksums before distributing the files.
When a registry release is authorized, publish the protocol crate before the
SDK because the SDK requires that exact protocol version. The packager only
creates and verifies artifacts; it never publishes them.

During implementation, a dirty public crate may be packaged only by adding
`--allow-dirty-sources`; such output is for clean-directory verification and
review, not registry publication. The script checks the public SDK/protocol
directories separately from unrelated host changes elsewhere in the checkout.

## Build from an extracted bundle

Use the pinned Rust toolchain from the bundle, then run the package-specific
commands at its root:

```sh
CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  cargo test --locked -p gamenode-plugin-protocol

CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target \
  cargo test --locked -p gamenode-plugin-sdk --lib --tests
```

These commands exercise only the public protocol and SDK packages. They do not
build the GameNode app, Core or platform crates.
