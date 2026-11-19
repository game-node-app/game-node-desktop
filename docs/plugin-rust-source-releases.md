# Plugin Rust source releases

The public Rust crates are `gamenode-plugin-protocol` and
`gamenode-plugin-sdk`. Their Cargo package versions identify source releases and
are independent of the wire API version. Keep the SDK's exact protocol crate
version requirement synchronized with the protocol package version.

## Prepare a release

Use Python 3.11+, Git and the Rust toolchain pinned in
[`rust-toolchain.toml`](../rust-toolchain.toml). Cargo packages against the
crates.io index and resolves the bundle lockfiles from locally available
registry metadata. Have registry access for packaging and fetching dependencies.

On the Linux development workstation, verify the external mount and target
before packaging. From the repository root, use a fresh output directory to
keep `SHA256SUMS` scoped to this release. The packager requires an absolute
`CARGO_TARGET_DIR` outside the repository; use the worktree's own target as in
[the development guide](development.md#required-cargo-target-location).

```sh
area=/data/DADOS_LINUX/geral/area_troca_rust
findmnt --mountpoint /data/DADOS_LINUX || exit 1
export CARGO_TARGET_DIR="$area/target-$(basename "$PWD")"
mkdir -p "$CARGO_TARGET_DIR"
export TMPDIR
TMPDIR=$(mktemp -d "$area/plugin-rust.XXXXXXXX") || exit 1
python3 scripts/package-plugin-rust-sources.py --output "$TMPDIR/artifacts"
```

The script requires clean public crate sources by default, checks Cargo's
effective target directory with `cargo metadata` before packaging, creates
versioned `.crate` packages, then extracts them into clean source workspaces on
the external mount. It resolves lockfiles offline, checks the resolved
dependency paths, and runs the protocol and SDK tests from the extracted
sources. Cargo build output stays in `CARGO_TARGET_DIR`. The packager removes
its staging workspaces after the run.

The output directory contains the registry-format `.crate` files and
versioned `.tar.gz` source bundles. Each bundle includes its `Cargo.lock`, the
pinned `rust-toolchain.toml`, package source and a generated third-party
license inventory. It copies license text files present in the resolved crate
sources and explicitly lists packages whose source archive does not provide a
license text file. The SDK source bundle includes the matching protocol
source as a sibling crate so it can build before either crate is published.
Its SDK `Cargo.toml` still declares `gamenode-plugin-protocol` by exact version;
the bundle root patches that registry package to the copy contained in the
bundle. For a source build before registry publication, use the SDK
`.tar.gz` bundle. The packager prepares the SDK's `.crate` with a local
protocol patch while the normalized SDK manifest specifies crates.io. That
candidate's internal lockfile reflects the patch. `SHA256SUMS` records hashes
for the four archives, and each source bundle's `release-manifest.json`
records the package versions, dependency requirements, toolchain and lockfile
hash.

## Review and publish

1. Record the source revision and build platform. In the output directory, run
   `sha256sum -c SHA256SUMS` and confirm it checks both `.crate` files and both
   `.tar.gz` files for the intended versions.
2. Review the normalized `Cargo.toml` inside each `.crate` and compare its
   sources with the intended public files at the release revision. The SDK
   manifest must declare the exact protocol version from crates.io. Check
   each source bundle's lockfile and toolchain pin against its release
   manifest. Confirm that the resolved GameNode crates are the public
   protocol and, in the SDK bundle, the SDK.
3. Review both MIT license texts and the
   [public plugin licensing record](plugin-public-licensing.md) against the
   files in the release. Review `THIRD-PARTY-NOTICES.md` against the locked
   dependency graph. For any entry without a copied license text, verify the
   terms and required notices from the corresponding upstream release before
   distributing the artifacts.
4. After release authorization, publish the protocol crate first. Confirm that
   its exact version resolves from crates.io, then package and publish the SDK
   from the reviewed revision with Cargo's normal registry workflow, with no
   local protocol patch.
   Distribute the two verified source bundles with `SHA256SUMS` and record the
   release revision alongside them.

During implementation, a dirty public crate may be packaged only by adding
`--allow-dirty-sources` for clean-directory verification. Regenerate release
artifacts from the reviewed revision with the default source check before
publishing.

## Build from an extracted bundle

Extract the matching `.tar.gz` into a fresh directory outside the repository.
Run each command from the root of its respective bundle so rustup selects the
pinned toolchain. On the Linux development workstation, use an external target
outside the extracted bundle:

```sh
area=/data/DADOS_LINUX/geral/area_troca_rust
export CARGO_TARGET_DIR="$area/target-plugin-rust-bundle"
cargo metadata --locked --no-deps --format-version 1
cargo test --locked -p gamenode-plugin-protocol
cargo test --locked -p gamenode-plugin-sdk --lib --tests
```

Run the metadata command in each bundle before testing. Confirm that
`target_directory` matches `CARGO_TARGET_DIR` and that the workspace contains
only the public protocol crate, plus the SDK in its own bundle.
