# Design

## Context

See `proposal.md` for motivation and `specs/desktop-plugins/spec.md` for observable behavior. The Tauri executable currently acquires the per-user desktop instance lock, builds the Engine through `bootstrap`, manages it in Tauri state, and exposes the React bridge through `invoke` and `engine:snapshot`. `gamenode-core` contains domain state machines and provider records; `gamenode-app` contains the Engine, adapters, and desktop composition. Both crates are linked into the same process.

The existing desktop IPC accepts only instance activation and authentication callbacks. A separate CLI/daemon prototype has a gRPC Client API, but it is not a plugin contract and its daemon shares the desktop profile lock. The isolated `tools/ipc-probe` demonstrates authenticated gRPC over Linux UDS; it does not validate a production Plugin API. The current Steam provider is bundled and the Engine invokes its scanner during the existing coordinated scan flow.

## Goals / Non-Goals

**Goals:**

- Make the Tauri application process the sole owner of the Engine, profile, Plugin API server, and plugin child processes.
- Provide one Linux vertical slice in which an external Steam plugin reports scan results to the Tauri host and the Engine preserves existing reconciliation and persistence behavior.
- Keep the Rust crate boundary between domain (`gamenode-core`) and application/composition (`gamenode-app`).
- Remove the unused product CLI/daemon path without changing the React command or snapshot-event contract.

**Non-Goals:**

- Run the Plugin API while the desktop application is closed or add a background Core daemon.
- Implement Windows plugin transport in this change. The existing Windows provider scan remains in-process until a later transport migration.
- Support community plugin installation, arbitrary third-party permission prompts, sandboxing, or plugin marketplace/distribution.
- Move GameNode REST credentials, SQLite ownership, canonical library reconciliation, or provider persistence into plugins.
- Replace the Tauri WebView IPC with the Plugin API or merge the `gamenode-core` and `gamenode-app` crates.

## Decisions

### Host the Plugin API inside the Tauri process

Run the gRPC server as an asynchronous service on the Tauri application's Tokio runtime. The server holds a narrow application adapter that can submit authorized plugin results to the Engine; it does not construct another Engine or open storage independently. The React WebView continues using Tauri `invoke` and `engine:snapshot`.

The API is a distinct `gamenode.plugin.v1` contract, not a renamed or expanded `gamenode.client.v1` API. A separate package and service boundary lets plugin capabilities evolve without exposing UI commands or making generated Protobuf types part of `gamenode-core`.

### Use an authenticated local session on Linux

Bind a private UDS in the current user's runtime directory, restrict the socket to the owner, and validate peer UID. The host starts only the packaged plugin needed for a scan and gives that process a temporary session credential outside command-line arguments. The plugin performs a version/capability handshake before exchanging typed scan messages. The credential is revoked when the plugin exits or its operation ends.

The API accepts the packaged official Steam plugin's read-only discovery capability in this first slice. It does not imply that a native plugin process is sandboxed: plugin code still runs with the user's OS privileges. The token and UID checks authenticate the local session and prevent accidental cross-session use; they are not a same-user sandbox.

Use the already validated Tonic/Protobuf/UDS combination as the Linux transport foundation. The existing single-instance IPC remains responsible for GUI activation and URI callbacks; do not overload its bounded one-way message protocol with plugin requests. Windows keeps its current in-process scanner until a later change defines and validates gRPC over named pipes.

### Preserve the Engine's scan contract

The plugin manager participates at the existing `RunScan` provider effect. For Linux Steam, the Engine schedules an async plugin operation instead of invoking the bundled Steam scanner locally. The host routes the plugin's typed records and terminal result into the existing `ProviderScanned` flow, so library reconciliation, account matching, storage queueing, and snapshot publication remain Engine-owned.

The host reuses the current scan coordinator: overlapping requests do not spawn duplicate Steam processes. A successful empty result is a successful scan. A plugin startup, handshake, timeout, or execution error becomes the provider failure already understood by the library state machine; it must not replace the previous successful records, and an immediate automatic restart is not attempted.

### Follow the application lifecycle

The plugin manager is owned by Tauri state. Plugins run only for requested work and are reaped when their operation ends. Application shutdown requests plugin termination and waits asynchronously for bounded cleanup; the host must not block the UI thread. Since the Tauri process owns the profile, its existing single-instance lock remains the only Engine/profile lock.

### Remove the prototype Client API from product binaries

Remove the CLI and daemon binary targets, shared daemon client, daemon-only Client API, and their packaging/documentation paths. The new Plugin API is not a compatibility layer for those commands. Keep `tools/ipc-probe` isolated as research evidence unless implementation proves it redundant. Mark the unstarted `connect-tauri-gui-to-daemon` plan superseded so it no longer appears to be the active target.

### Preserve crate boundaries

Keep domain models and state machines in `gamenode-core`; keep Engine coordination, OS composition, Tauri commands, plugin process management, and the Plugin API host in `gamenode-app`. Calls from the Plugin API adapter to the Engine are ordinary in-process Rust calls, not another transport layer.

## Risks / Trade-offs

- [The GUI must be running for plugins to connect] → This is the selected lifecycle; plugin processes are children of the desktop application and are stopped with it.
- [A same-user native plugin can still access user files or start processes directly] → State the trust limit clearly, bundle only the official Steam plugin in this slice, and avoid presenting IPC authentication as sandboxing.
- [Plugin failure could erase valid cached installations] → Feed failures through the existing provider failure path, which preserves the previous successful provider data.
- [Plugin records may drift from the established scanner shape] → Reuse the existing provider record conversion and compare Steam ids, titles, and install directories in the vertical-slice acceptance checks.
- [The old daemon and Tauri currently use the same profile lock] → Remove the daemon product target before any packaged coexistence; retain Tauri as the sole owner.
- [Linux transport evidence does not validate Windows named pipes] → Keep Windows on its existing in-process scanner and defer its Plugin API transport.

## Migration Plan

1. Mark the unstarted GUI-to-daemon proposal superseded and remove the CLI/daemon Client API from product targets and packaging; keep the current Tauri Engine path as the sole product host.
2. Add the Plugin API server, per-plugin session handshake, allowlist, process manager, and Linux UDS endpoint to the Tauri host.
3. Move Linux Steam discovery behind the plugin manager while retaining Windows' existing scanner and the Engine's current result/persistence behavior.
4. Package the official Linux Steam plugin with the desktop application, verify startup/failure/recovery and scan equivalence using an isolated profile, then update development and architecture docs.

Rollback restores the previous desktop binary and built-in Steam scanner. No database migration or user-data rewrite is intended. Do not run the old daemon against the profile while the Tauri application owns it.

## Open Questions

None that change this first Linux Steam discovery slice. The transport and authorization model for Windows and community plugins remain explicitly out of scope.
