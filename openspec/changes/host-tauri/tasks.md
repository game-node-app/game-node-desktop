# Tasks

## 1. Retire the CLI/daemon prototype and stale direction

- [x] 1.1 Remove the `gamenode-cli` and `gamenode-daemon` product targets, Client API proto/service, and shared daemon client code; verify `cargo metadata --no-deps` lists only the desktop binary and the locked app check succeeds.
- [x] 1.2 Remove CLI/daemon packaging and development instructions, update architecture documents, and retire `connect-tauri-gui-to-daemon`; verify repository searches and `openspec list` show no obsolete active implementation path.

## 2. Plugin API hosted by Tauri

- [x] 2.1 Add a separate versioned `gamenode.plugin.v1` Protobuf contract and transport crate; verify generated client/server code compiles independently of Tauri.
- [x] 2.2 Implement the Linux Plugin API server in the Tauri Tokio runtime on a private per-user UDS with peer UID validation, bounded frames/connections, per-launch credentials, protocol negotiation, and capability handshake; verify accepted and rejected handshake cases in API tests.
- [x] 2.3 Add host-owned plugin process management for authorized discovery work, including startup timeout, operation deadline, exit reporting, cleanup, and asynchronous shutdown; verify lifecycle behavior without blocking the Tauri UI thread.
- [x] 2.4 Document the same-user trust boundary and Linux-only transport support; verify no GameNode credential or callback URL is included in plugin messages or diagnostics.
- [x] 2.5 Replace Steam-specific host configuration with a provider-keyed descriptor registry and operation-scoped capabilities; verify unknown providers and cross-provider/operation results are rejected.

## 3. Reusable scanner contract and Steam migration

- [x] 3.1 Extract shared scanner contract and Steam discovery logic into a crate independent of `gamenode-core` and `gamenode-platform`; verify dependency tree and unit cases for valid, empty, and malformed Steam entries.
- [x] 3.2 Adapt both in-process and plugin Steam scanners to the shared implementation; verify identical provider ids, names, and install directories.
- [ ] 3.3 Route Engine scan effects through the generic plugin registry on Linux while retaining Windows' in-process Steam path; verify coalescing, previous-data preservation, persistence, and explicit unsupported-transport behavior.
- [ ] 3.4 Preserve the React command and `engine:snapshot` contracts; verify existing UI bridge checks and the Linux desktop build.

## 4. Linux package-manager discovery plugin

- [x] 4.1 Implement native pacman ownership queries on Arch-based distributions and PackageKit installed-package querying elsewhere, with conservative game classification from installed desktop-entry metadata; verify backend selection, game/non-game classification, empty and malformed owner output, deduplication with deterministic tests, and the live SuperTux case on Arch.
- [x] 4.2 Register the Linux package manager plugin with the shared `library.discovery` capability and map typed candidates through Engine reconciliation; verify its failures preserve prior results and do not block Steam scans.
- [x] 4.3 Document scanner authoring, shared result contract, permission scope, distro backend selection, and Linux dependency behavior; verify documented permissions match host registration.

## 5. Package and accept the Tauri-hosted plugin flow

- [x] 5.1 Package the Linux Steam plugin with the desktop application and resolve its executable path without PATH lookup or downloaded code; verify a packaged isolated-profile launch starts it only for a requested scan.
- [ ] 5.2 Package both Linux discovery plugins; verify executable resolution, permissions, and that neither plugin links the internal Core/platform crates.
- [ ] 5.3 Run Linux acceptance with an isolated profile: successful and empty Steam scans, SuperTux candidate through pacman on Arch and PackageKit on a supported non-Arch host, plugin missing/timeout/failure recovery, GUI responsiveness, clean shutdown, and no credential leakage; record environment limitations and results in development guidance.
- [ ] 5.4 Run locked Rust checks, relevant Rust and UI bridge tests, packaging checks, and strict OpenSpec validation; verify product binaries no longer contain the CLI/daemon prototype.
