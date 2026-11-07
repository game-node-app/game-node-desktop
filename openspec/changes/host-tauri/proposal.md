# Proposal

## Why

The Linux CLI/daemon prototype adds a second application host without a clear product benefit, while the desktop application already owns the Engine and profile. Plugin processes still need a stable, authenticated way to submit work to that application, so Tauri should host the Plugin API directly.

## What Changes

- Keep one desktop process as the owner of the Engine, profile, persistence, and plugin lifecycle. Keep `gamenode-core` and `gamenode-app` as separate Rust crates compiled into the same application.
- Add a versioned Plugin API hosted by the Tauri application process and distinct from Tauri's React `invoke` IPC. Plugins connect to the host and submit typed requests or observations; the host validates them and routes accepted work through the Engine.
- Deliver a first Linux plugin flow for Steam discovery, preserving the existing records, scan coordination, failure behavior, and persistence semantics.
- Authenticate each plugin session, validate its declared identity and capabilities, and avoid giving plugins GameNode credentials or direct database ownership.
- Remove the product CLI/daemon prototype and its Client API. Retain the isolated IPC probe as transport research unless it becomes obsolete during implementation.
- Supersede the unstarted `connect-tauri-gui-to-daemon` change. Windows plugin transport and non-Steam plugins remain future work.

## Capabilities

### New Capabilities

- `desktop-plugins`: The desktop host authenticates, authorizes, and routes requests from separately launched plugins through a versioned local API.

### Modified Capabilities

None. Existing provider-facing scan results and failure semantics remain required; this change adds a plugin host without changing those product behaviors.

## Impact

- Rust: Tauri startup and Engine composition, provider scan orchestration, plugin process lifecycle, IPC transport, and removal of the CLI/daemon binaries and Client API.
- Packaging and docs: install the Linux Steam plugin with the desktop host and document plugin failure/recovery behavior.
- React: keep the existing Tauri command and `engine:snapshot` contracts; plugins do not connect to the WebView IPC.
- Data: no database schema change is intended; existing library records, credentials, and settings remain owned by the desktop application.
