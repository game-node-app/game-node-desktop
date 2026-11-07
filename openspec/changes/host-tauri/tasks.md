# Tasks

## 1. Retire the CLI/daemon prototype and stale direction

- [x] 1.1 Remove the `gamenode-cli` and `gamenode-daemon` product targets, Client API proto/service, and shared daemon client code, including the current uncommitted extraction; verify `cargo metadata --no-deps` lists only the desktop binary and the locked app check succeeds.
- [x] 1.2 Remove CLI/daemon packaging and development instructions, update `docs/plugin-architecture-plan.md` and `docs/core-cli-contract-proposal.md` to identify the new Tauri-hosted direction, and retire `connect-tauri-gui-to-daemon` as superseded; verify repository searches and `openspec list` show no obsolete active implementation path.

## 2. Plugin API hosted by Tauri

- [x] 2.1 Add a separate versioned `gamenode.plugin.v1` Protobuf contract and transport crate, without adding protocol types to `gamenode-core`; verify generated client/server code compiles independently of Tauri.
- [x] 2.2 Implement the Linux Plugin API server inside the Tauri Tokio runtime on a private per-user UDS, with peer UID validation, bounded frames/connections, per-launch credentials, protocol negotiation, and capability handshake; verify accepted and rejected handshake cases in API tests.
- [x] 2.3 Add a host-owned plugin registry/allowlist and child-process manager for the packaged Steam discovery capability, including startup timeout, operation deadline, exit reporting, cleanup, and asynchronous shutdown; verify lifecycle behavior without blocking the Tauri UI thread.
- [x] 2.4 Document the same-user trust boundary and Linux-only transport support, and add host-level security/diagnostic guidance; verify no GameNode credential or callback URL is included in plugin messages or diagnostics.

## 3. Connect Steam discovery to the existing Engine

- [x] 3.1 Implement the Linux Steam plugin executable using the existing scanner and provider record semantics; verify ids, titles, install directories, empty results, and malformed-entry handling against the bundled scanner behavior.
- [x] 3.2 Route Linux Steam `RunScan` work through the host plugin manager and feed typed completion/failure into the existing Engine provider result path; verify coalesced requests, previous-data preservation on failure, and confirmed persistence with Engine/daemon-free integration tests.
- [ ] 3.3 Keep Windows Steam discovery on its current in-process scanner and make unsupported plugin transport explicit; verify Windows builds retain the existing provider composition.
- [x] 3.4 Preserve the React command and `engine:snapshot` contracts and update development guidance for the hosted plugin flow; verify existing UI bridge checks and the Linux desktop build.

## 4. Package and accept the Tauri-hosted plugin flow

- [x] 4.1 Package the Linux Steam plugin with the desktop application and resolve its executable path without PATH lookup or downloaded code; verify a packaged isolated-profile launch starts the plugin only for a requested scan.
- [ ] 4.2 Run Linux acceptance with an isolated profile: one Tauri Engine/profile owner, successful and empty Steam scans, plugin missing/timeout/failure recovery, GUI responsiveness, clean host shutdown, and no credential leakage; record results in development guidance.
- [x] 4.3 Run locked Rust checks, relevant Rust and UI bridge tests, packaging checks, and strict OpenSpec validation; resolve regressions and verify the product binaries no longer contain the CLI/daemon prototype.
