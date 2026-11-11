# Public plugin code licensing record

This record defines the public-code boundary for the optional plugin SDK and
wire protocol. It does not change the license of GameNode Desktop, Core, the
official plugin implementations, or their assets.

## Files in the public boundary

| File | Source / authority | Release license |
| --- | --- | --- |
| `crates/plugin-protocol/Cargo.toml` | GameNode project metadata; approved by project owner | MIT |
| `crates/plugin-protocol/build.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/proto/plugin.proto` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/catalog.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/capability.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/lib.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/login.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/models.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/wire.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/settings.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/src/settings/wire.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/tests/contract.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-protocol/tests/settings.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-sdk/Cargo.toml` | GameNode project metadata; approved by project owner | MIT |
| `crates/plugin-sdk/src/lib.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-sdk/tests/reference_host.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |
| `crates/plugin-sdk/tests/settings_reference_host.rs` | First-party; authored by Richelmy Monteiro in commit `5e9a882` | MIT |

No Go or Rust example source existed in the repository when this audit was
performed. Each example file introduced later must be recorded here with its
source/authority and license before inclusion in a public package.

These files are first-party GameNode implementation. The repository history
attributes their existing source to Richelmy Monteiro in commit
`5e9a882476cbfde7e4b272364e5cac050cd160fb`; the current tree contains no
vendored implementation source in either crate. Generated protobuf Rust is
built from the first-party `.proto` definition and is not checked in. The
project owner authorized the public SDK/protocol MIT boundary in the
`add-third-party-plugins` change. New contributors to these paths must agree to
the same license before their contributions are included in a public release.

## License boundary

The protocol and SDK crates carry MIT licenses, each accompanied by its
`LICENSE` text. Public source archives must include both license texts and
third-party dependency notices. Workspace application code, Core, platform
adapters and official plugins remain GPL-3.0-only under the root `LICENSE`.
No GPL host implementation file is included in the standalone SDK/protocol
archives. Third-party dependencies retain their upstream licenses; this record
does not relicense them.

At the API 1.3 baseline, `cargo metadata --offline --format-version 1` resolved
160 packages in the combined protocol and SDK dependency closures. Every
resolved package reported license metadata. The closure includes MIT and
Apache-2.0 choices and compatible notices such as Unicode-3.0, BSD-3-Clause,
Zlib, Unlicense, and the LGPL-2.1-or-later alternative on `r-efi`. This check
reviews declared expressions, not the full text or binary contents of every
dependency license. Release tooling must include upstream notices and repeat
the audit against the exact archive dependency graph.

The current protocol and SDK dependency trees are declared in their respective
Cargo manifests and `Cargo.lock`. Release packaging must generate and review a
dependency-license inventory for the resolved target before publishing an
archive. If a dependency's license or source cannot be verified, that
dependency must be removed or replaced before the affected archive is
published.

After the API 1.4 protocol/SDK additions, the current locked closure resolves
168 packages, again with no missing Cargo license expressions. The new
dependencies remain within the same reviewed permissive/Unicode-compatible
license families. This updated count was checked with
`cargo metadata --locked --offline --format-version 1` on 2026-11-10.
