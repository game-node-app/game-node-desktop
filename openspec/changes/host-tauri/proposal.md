# Proposal

## Why

The Tauri application is the sole owner of the Engine and profile, while separately launched plugins need a narrow, authenticated way to contribute discovery results. The current Steam-only path couples the host and plugin to Steam-specific logic, making another scanner unnecessarily dependent on Steam and internal Core/platform APIs.

## What Changes

- Keep `gamenode-core` and `gamenode-app` as separate Rust crates compiled into the desktop process, with Tauri owning the Engine, profile, persistence, and plugin lifecycle.
- Generalize the Linux Plugin API host around registered discovery providers, scoped capabilities, and typed provider results that flow through the existing Engine reconciliation path.
- Extract the reusable scanner contract and Steam implementation so the Steam plugin no longer depends directly on `gamenode-core` or `gamenode-platform`.
- Add an official Linux package-manager discovery plugin that identifies installed packages as game candidates, using pacman directly on Arch-based distributions and PackageKit elsewhere; the Engine remains responsible for validation, confirmation, and persistence. Use installed SuperTux as an acceptance reference.
- Preserve the existing Tauri WebView IPC, retire the product CLI/daemon prototype, and keep Windows Steam scanning in-process pending Windows plugin transport.
- Document scanner authoring, discovery permissions, native package-manager selection, and candidate limitations.

## Capabilities

### New Capabilities

- `desktop-plugins`: The desktop host authenticates, authorizes, and routes results from separately launched discovery plugins through a versioned local API.

### Modified Capabilities

None. Existing provider-facing scan results and failure semantics remain required; this change generalizes their plugin source without changing the UI contract.

## Impact

- Rust: scanner contracts, Steam scanner extraction, Tauri Engine composition, plugin registry/process lifecycle, Linux package-manager integration, and the Plugin API.
- Dependencies and runtime: native pacman queries on Arch-based distributions and PackageKit D-Bus access elsewhere; scanner absence or unsupported environments must produce a provider-level result without blocking other scans.
- Packaging and docs: include both official Linux discovery plugins and document permissions and candidate semantics.
- Data: no database schema change; Engine owns candidate review, canonical records, credentials, and persistence.
