# Proposal

## Why

Current mainline includes the PC ports catalog and installer, while a separate integration branch implements Steam through the Plugin API. Bring the complete Steam plugin into the current application without coupling acceptance to a PC ports redesign or importing unrelated plugin features.

## What Changes

- Use `origin/main` as the application baseline and selectively transplant the Steam plugin and its required host infrastructure from `integration/migrate-steam-to-plugin` into a new integration branch. Preserve both source branches and existing contributor work.
- Delegate every Steam-specific operation to the official plugin on Linux and Windows: installed discovery, login/Guard, session validation and rotation, account workflow, owned/Family catalog, action declarations and store playtime counters.
- Retain GameNode authentication, library reconciliation, persistence, matching, favorites, covers, process tracking and synchronization in Core/Engine. Expose only scoped host services through the public protocol and SDK.
- Integrate generic account controls and trusted Steam plugin inventory into the existing GUI, keeping current mainline navigation, settings and PC ports behavior.
- Package the Steam executable for development and releases on both platforms, with private Unix sockets on Linux, secure named pipes on Windows and a declared build-host Protobuf compiler.
- Preserve schema 8, existing Steam credential locations and all library/ports identities and data. Revalidate lifecycle fixes for socket cleanup and closing a login WebView during cookie collection.
- Require automated integration checks, real Linux acceptance and explicit user confirmation before native Windows VM acceptance. Cross-compilation alone cannot establish Windows parity.
- Defer the PC ports plugin, additional catalog/install capabilities, Linux package-manager plugin, optional declarative plugin settings, new stores and marketplace work. Existing ports implementation and platform scope remain the baseline.

## Capabilities

### New Capabilities

- `desktop-plugins`: Authenticated local runtime, independent public boundary, supervised lifecycle, trusted inventory, packaging and isolated failure behavior. Reuses the capability path proposed by the unsynchronized predecessor changes, with Steam-only delivery scope.
- `desktop-plugin-services`: Explicit provider-scoped services for context, discovery, account state, owned/Family catalog, isolated login extraction, secrets, counters and action declarations. Consolidates the predecessor's required Steam service contract without its optional settings extension.

### Modified Capabilities

- `desktop-providers`: Permit registered plugin scan/catalog/counter backends and require the complete Steam integration to use them on Linux and Windows while preserving existing provider semantics and mainline compatibility.

## Impact

- Source baseline inspected: mainline `578ffa72687847f95b781d6862a7fdb1dc05e390`; implementation donor: `77389ce1fb3a4e218ab4fbe3504b115883ea09de`. Re-fetch and record the actual selected mainline SHA during implementation.
- Affects workspace dependencies, protocol/SDK/Steam crates, provider descriptors, host services, Engine composition, platform transport/credentials, frontend account/inventory adapters, CI and Steam packaging. Shared PC ports wiring may need composition adjustments; its domain, installer, UI behavior, overlay and migration are preserved.
- `migrate-steam-to-plugin` supplies implementation and historical evidence; `host-tauri` supplies earlier runtime context. This change owns the new integration checklist. Do not copy completed checkboxes or apply competing predecessor spec deltas over the consolidated result; retain deferred work and historical evidence explicitly.
- The existing `add-pc-port-catalog` change from mainline and its pending checks remain intact. Readiness requires verifying both Steam and ports; no application code, branch/history change, commit, push or merge is performed by this proposal.
