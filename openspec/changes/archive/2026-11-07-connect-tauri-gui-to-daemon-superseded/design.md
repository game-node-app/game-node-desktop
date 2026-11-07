# Design

## Context

See `proposal.md` for motivation and `specs/` for externally visible behavior. The Linux daemon currently owns the Engine, data paths, credentials, and a per-user IPC lock; its gRPC API is UDS-only and covers CLI operations. The Tauri host still bootstraps its own Engine, acquires that same lock, and publishes `Snapshot` values to React through `engine:snapshot`. The React API is expressed as Tauri commands in `ui/src/api/commands.ts`; its query cache already accepts full snapshots. The Tauri command surface also includes native file pickers and a custom asset protocol, while the Engine contains local-game, save, backup, and tool workflows absent from the current gRPC API.

## Goals / Non-Goals

**Goals:**

- Make the Linux daemon the only owner of Core state and the profile while allowing the Tauri GUI and CLI to use it together.
- Preserve the existing React command and snapshot-event contracts where feasible.
- Preserve native dialogs, window management, asset URLs, callback safety, and the Linux close/quit contract.
- Keep the gRPC API local, versioned, responsive, and recoverable after a daemon restart.

**Non-Goals:**

- Add Windows daemon transport or migrate the Windows Tauri host in this change. Windows named pipes are the planned later transport; AF_UNIX is not used because the current stable Tokio APIs expose Unix streams only on Unix.
- Redesign React views, replace Tauri, or change library/auth/settings behavior.
- Add plugin runtime, remote library browsing, or new user-facing features.

## Decisions

### One profile owner, separate GUI instance

Keep the daemon's existing profile lock and endpoint as the authority for Engine/data ownership. Give the Tauri GUI a separate per-user single-window endpoint so starting the GUI no longer races the daemon for the profile lock. Extract daemon discovery/start/readiness and gRPC connection into a shared Rust client module used by both CLI and Tauri; the GUI starts the sibling daemon only when no compatible daemon is available.

The GUI remains a single instance. A normal second launch asks the existing GUI to focus. An incoming `app.gamenode://` URL is sent to the daemon's existing callback IPC path for Core validation, then the GUI is raised. If no daemon exists, the URI-launch path starts it before forwarding the callback. Callback URLs are never logged. This avoids adding a second auth state machine or exposing callback credentials to the React layer.

On Linux, `CloseRequested` and Quit use an asynchronous graceful daemon stop and exit the GUI only after acceptance. Stop refusal during an active scan keeps the GUI available and reports the refusal; close handling must not block the Tauri main thread. The CLI's explicit daemon stop keeps its existing semantics. `--background` remains a Windows-only host behavior in this change.

### Preserve the frontend bridge with a Tauri gRPC adapter

Keep `commands.ts`, `invoke()` command names, and the `engine:snapshot` event as the first migration boundary. The Tauri process owns a shared `DaemonClient`; each existing domain command becomes an async adapter call. The daemon implements the missing API for every currently exposed UI operation, grouped by auth/settings, library/account, local games/saves, and tools. Do not let the GUI import or bootstrap `Engine` on Linux, and do not let command adapters read or write the profile database directly.

Keep host-only work in Tauri. App metadata, license notices, geometry, tray/window operations, and native file/folder dialogs remain local. For commands that currently combine a picker with a Core mutation, Tauri obtains the selected path and sends the resulting domain request to the daemon. The daemon remains responsible for validation, state changes, and persistence.

### Stream a versioned serialized Core snapshot

Add `GetSnapshot` and server-streaming `WatchSnapshots` RPCs. Wrap the existing serde-serialized, credential-free `Snapshot` JSON in a protobuf envelope with an API/schema version and monotonic revision. This avoids duplicating the large Rust snapshot model as protobuf fields while matching the current TypeScript shape. Validate deserialization at the Tauri boundary; a version/schema mismatch is a visible connection error, never a partial default snapshot.

The stream sends a full snapshot first, then full coalesced snapshots with increasing revisions. The Tauri host drops duplicate or older revisions and emits the newest payload as `engine:snapshot`. On disconnect it reconnects with bounded backoff; a fresh stream always starts with a full snapshot, so events missed during downtime cannot leave React Query permanently stale. The stream owns no mutable state and includes no session credential or callback URL.

### Version the API independently from the application build

Expose a protocol major/minor version in daemon status. The GUI checks compatibility before bootstrapping; a major mismatch produces an actionable error and does not execute commands. Minor additions remain backward-compatible. Keep the package version for display and diagnostics, but do not treat it as the protocol contract. A stale daemon launched from an earlier installation must be detected; the client may request its graceful stop and launch the bundled daemon only when that stop succeeds.

### Keep the custom asset protocol at the GUI boundary

Continue serving cover files from the shared cover cache through the existing asynchronous `gamenode-asset://` protocol. Dynamic in-memory artwork currently comes from `Engine::artwork`; add an artwork read RPC and call it from the async asset handler when the disk cache misses. Avoid reading artwork through a new synchronous path or moving byte payloads into every snapshot. Preserve current URL, ETag, cache, and path-validation behavior.

### Use the existing local gRPC transport on Linux

Keep UDS under the private per-user runtime directory, socket mode `0600`, and peer UID validation. Share connection setup so CLI and Tauri cannot drift on endpoint selection or identity checks. Windows remains on the current in-process host until a separate change supplies named-pipe gRPC IO with a current-user ACL and validates native Windows lifecycle behavior.

## Risks / Trade-offs

- [The full Tauri command surface is much broader than the current CLI API] → Inventory every registered command and migrate by domain; acceptance requires no GUI feature silently falling back to a second Engine.
- [Snapshot JSON can drift from TypeScript types] → Version the envelope, keep Rust serialization and TS fixtures aligned, and reject unknown major schemas.
- [A daemon restart can interrupt a stream or command] → Reconnect streams from a full snapshot, bound command deadlines, and show retryable connection state in the existing shell.
- [GUI startup and daemon startup can race, or two hosts can claim profile ownership] → Keep one daemon profile lock, separate GUI lock namespace, and test simultaneous CLI/GUI startup.
- [Native dialogs currently call Engine-backed commands] → Split selection from mutation so file chooser callbacks stay in Tauri while all Core writes go through gRPC.
- [Stopping during a scan is refused] → Keep the UI responsive and open, surface a localized refusal, and let the user retry after the scan finishes.
- [Dynamic artwork is not always on disk] → Add async artwork retrieval from the daemon and retain disk-cache fallback.
- [Windows remains on the direct Engine backend temporarily] → Keep platform-specific composition explicit and track Windows named-pipe migration as a separate change; do not imply the Linux gRPC endpoint works there.

## Migration Plan

1. Add the full snapshot/stream and missing domain RPCs to the daemon while retaining CLI compatibility.
2. Extract shared daemon launch/connect logic and add the separate Linux GUI instance endpoint and callback forwarding.
3. Switch Linux Tauri commands, snapshot events, and artwork protocol to the daemon client; preserve a direct Engine composition only for Windows until its transport change.
4. Package the desktop, CLI, and daemon together. Exercise clean startup, existing-daemon attach, simultaneous CLI and GUI, URI callbacks, scan-driven updates, close/quit, and daemon restart with a disposable profile.
5. Rollback uses the previous Tauri binary and existing data format; no database migration is introduced. Stop the new daemon before launching the old direct-Engine Tauri binary to avoid two profile owners.

## Open Questions

None that change this scope. The exact backoff intervals and protobuf minor-version policy can be settled during implementation without changing the behavior contract.
