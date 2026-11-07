# Tasks

## 1. Shared client and snapshot API

- [ ] 1.1 Extract daemon discovery, startup, readiness, endpoint validation, and gRPC connection into a shared Rust client module; verify CLI builds and its existing daemon lifecycle commands still work.
- [ ] 1.2 Add protocol major/minor negotiation and actionable mismatch errors; verify compatible and incompatible client/server pairs in API tests and document the version contract.
- [ ] 1.3 Add full snapshot retrieval and a revisioned snapshot stream with initial-state delivery, coalescing, and reconnect support; verify no stale final snapshot after subscribe/reconnect using daemon integration tests.
- [ ] 1.4 Add an inventory of all registered Tauri commands and their Core/host ownership, then map every Core operation to a typed/versioned gRPC RPC; verify the inventory covers `commands.ts` and the Tauri invoke handler.

## 2. Linux GUI and daemon lifecycle

- [ ] 2.1 Give the Linux GUI a separate per-user single-instance endpoint while retaining the daemon's profile lock; verify simultaneous CLI/GUI startup creates one daemon, one GUI, and one profile owner.
- [ ] 2.2 Make Linux Tauri connect to an existing daemon or start the bundled daemon with readiness and failure handling; verify clean launch, attach to an existing daemon, and daemon-start failure in isolated-profile integration tests.
- [ ] 2.3 Route incoming `app.gamenode://` callbacks to the daemon and raise the existing GUI without logging callback contents; verify cold-start, pending-login, malformed, and duplicate callback paths with a local auth server.
- [ ] 2.4 Route Linux close and Quit through asynchronous graceful daemon shutdown, keeping the UI responsive and handling active-scan refusal; verify storage flush, refusal, and clean process exit with lifecycle tests and update development guidance.

## 3. Tauri state and command bridge

- [ ] 3.1 Store a shared daemon client in the Linux Tauri host and relay versioned snapshots to `engine:snapshot`; verify React Query receives initial state, newer revisions, and refreshed state after reconnect without changing the frontend command contract.
- [ ] 3.2 Migrate authentication, sync/playtime settings, window geometry persistence, and library snapshot/rescan commands to gRPC; verify each UI command changes daemon state and reflects it in a second connected client.
- [ ] 3.3 Migrate library selection, matching/search, membership, add/remove, play/install, and status commands to gRPC; verify accepted and rejected operations preserve existing Core behavior and report state through snapshots.
- [ ] 3.4 Keep native file/folder pickers in Tauri and send selected paths to daemon-owned local-game, save, backup, and discovery operations; verify cancellation makes no mutation and successful selection updates the shared snapshot.
- [ ] 3.5 Migrate game-tools state and operations to gRPC while keeping OS windowing and external URL opening in Tauri; verify tool state survives client reconnect and existing tools flows remain accessible.

## 4. Assets and distribution

- [ ] 4.1 Add async artwork retrieval from the daemon to the Tauri asset protocol while preserving shared cover files, URL forms, ETags, and path validation; verify cached, in-memory, missing, and invalid artwork requests.
- [ ] 4.2 Package the Linux desktop, CLI, and daemon together and document GUI startup, daemon lifecycle, and recovery; verify the packaged GUI starts or attaches to its sibling daemon from an isolated profile.

## 5. End-to-end acceptance

- [ ] 5.1 Run the Linux acceptance flow with a disposable profile: authenticate, use GUI and CLI concurrently, scan, observe updates, exercise library/local-game/tools flows, restart the daemon, and close/quit; verify one Engine/profile owner and no credential leakage in diagnostics.
- [ ] 5.2 Run the relevant locked Rust checks and UI bridge checks, resolve regressions, and verify `openspec validate --change connect-tauri-gui-to-daemon --strict` passes.
