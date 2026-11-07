# Proposal

> Superseded before implementation on 2026-11-07. The desktop Tauri process
> remains the Engine owner and hosts the Plugin API. See the active `host-tauri`
> change; this proposal is retained only as historical context.

## Why

The Linux Tauri application still owns an Engine directly, while the new CLI uses a separate daemon. They cannot safely share the same profile or live application state. Making the daemon the sole Engine owner lets the GUI and CLI use one authenticated session, library, tracker, and settings state.

## What Changes

- Connect the Linux Tauri host to the per-user daemon over the existing local gRPC/Unix-socket transport.
- Extend the daemon API to provide the complete UI snapshot, state updates, and the domain operations needed by the existing GUI.
- Keep Tauri as the native shell and adapt its current commands and snapshot events to the daemon, preserving the React-facing bridge where possible.
- Separate GUI single-instance handling from daemon/profile ownership, and preserve callback delivery and Linux close/quit behavior.
- Keep Windows on its current in-process Engine path in this change. Plan Windows daemon transport separately using named pipes with current-user access restrictions.

## Capabilities

### New Capabilities

- `desktop-daemon`: A per-user daemon owns the Engine and exposes its state and operations to local clients through a restricted, versioned API.

### Modified Capabilities

- `desktop-shell`: On Linux, the Tauri shell obtains application state and performs domain actions through the daemon rather than owning a second Engine.

The existing `desktop-lifecycle` requirements remain behaviorally unchanged: daemon ownership and shutdown are specified under the new capability, while GUI single-instance behavior and callback delivery continue to meet the existing contract.

## Impact

- Rust: `crates/app/src/main.rs`, `commands.rs`, `daemon.rs`, `proto/client.proto`, bootstrap and IPC lifecycle.
- React: retain the `engine:snapshot` and Tauri command contracts where practical; add revision handling only if needed for reconnect ordering.
- Assets: preserve `gamenode-asset://`; bridge dynamic artwork from the daemon while covers remain served from the shared cache.
- Packaging: Linux desktop, daemon and CLI binaries must be installed and discoverable together.
- Windows: no daemon transport or GUI migration in this change; a later change can use Tokio named pipes and restrict access to the current user.
