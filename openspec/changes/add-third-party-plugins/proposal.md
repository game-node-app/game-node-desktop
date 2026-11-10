# Proposal

## Why

GameNode already has a local gRPC provider protocol and Rust SDK, but only trusted bundled executables can use them. Third-party developers need a documented, independently implementable contract and users need explicit control over installation, host permissions, execution and updates.

## What Changes

- Add a separate, managed third-party plugin registry on Windows and Linux, with installation, activation, disabling, removal and failure isolation. Existing official plugins, especially Steam, retain their descriptors, behavior, credentials and packaging.
- Define a versioned package manifest, per-file integrity checks, publisher signatures and identity pinning, platform selection, signed update metadata, compatibility checks and atomic update/rollback.
- Add persistent, independently revocable API permissions, scoped resources, explicit machine-readable denials and permission-change notifications. Native processes remain unsandboxed; distinguish enforced API grants from declared native access and explain that GameNode cannot confine direct OS access.
- Publish a finite initial resource/permission catalog and implement its host RPCs for library, metadata, favorites, artwork, launching, scoped storage/network/files, notifications, backups, tools and PC ports. Preserve domain validation and confirmations for actions that affect files or execute software.
- Add `game.launch` for unchanged detected URI/executable targets and `game.custom_launch` for approved alternate executables, arguments, working directories, runtime wrappers and environment overrides. Launch extensions can target user-approved games from any provider, including official providers, without modifying those providers.
- Expose multiple-library/root registration and independent game identities for content executed by shared emulators, interpreters or engines.
- Add save-profile configuration and common location inspection for saves/installations/runtimes: effective access, size, dates, hashes/comparison, mount availability and storage characteristics. Accept plugin-produced benchmark/SMART reports without executing diagnostic tools in Core.
- Accept, retain as authorized and relay hardware, in-game presence and game-performance observations (FPS, CPU, RAM). API data ingestion/query/subscriptions ship independently of future Core dashboards, predictions or public-profile integration.
- Pair every shareable data-ingestion family with scoped read/list/subscription access for other plugins. Sharing defaults off; users select fields, source, destination and delivery/retention, with plugin/developer/publisher/version provenance.
- Add independent `storage.persist` consent, plugin-attributed disk usage, uninstall retain/delete choices and API-only own-namespace cleanup/retention policies without exposing physical paths to plugins. Persistent game-performance history can be reused by explicitly authorized current or future plugins.
- Establish API negotiation, support/deprecation rules and capability-based fallback for third-party clients, without retroactively applying third-party compatibility policy to official plugins.
- Prepare independently consumable MIT SDK/protocol/example distributions, conditional on documented copyright authority; keep the core and host GPL-3.0-only. SDK use is optional.
- Deliver two practical placeholder plugins: Go using generated gRPC bindings directly without the GameNode SDK, and Rust using the SDK. Cover real requests, responses, streams, standard/custom launch recipes, denial and compatibility handling against the same host.
- Let plugins own their functional configuration window, opened or focused through Configure in GameNode. Keep declarative forms optional; installation, permissions, activation, updates and licenses stay in GameNode.
- Extend settings with third-party installation, permissions, effective capabilities, native-access warnings, version/update status and offline license notices. Validate declared license metadata and distribution boundaries without claiming automatic legal certification.

## Capabilities

### New Capabilities

- `desktop-external-plugins`: Managed third-party installation, publisher identity, integrity, lifecycle, updates, persistent-data accounting and cleanup.
- `desktop-plugin-permissions`: Persistent scoped grants, field-level data-sharing and retention consent, explicit denials, revocation and accurate disclosure of native access.
- `desktop-plugin-api`: Language-neutral data/resource contracts, inter-plugin exchange, save/storage inspection, libraries/runtimes, performance/presence, game launch and version compatibility.
- `desktop-plugin-development`: MIT public development assets, documentation and Rust/Go interoperability examples.

### Modified Capabilities

- `desktop-providers`: Permit validated third-party provider registrations and local executable targets, plus independent launch extensions, while preserving bundled manifest trust and reserved identities.
- `desktop-settings`: Add third-party management, Configure entry point and license/permission disclosure to the existing plugin inventory in Stores.

## Impact

Shared protocol and SDK additions; new third-party host/registry/package/update services beside `crates/app/src/plugin_host*`; domain adapters in app/core; persistence and OS adapters in platform; async Tauri commands; existing React `PluginInventory`, query bridge, mock and translations; public developer docs, examples and packaging tooling. Proposed dependencies include archive/hash/signature/SPDX tooling and Go gRPC plus a Windows named-pipe adapter for the standalone example.

This change plans no official plugin migration, marketplace, arbitrary plugin JavaScript in the UI, kernel/OS sandbox or direct database/Core credential access. The active Steam integration and Linux package-manager changes overlap shared host code; their official behavior and API 1.1–1.3 contracts are compatibility constraints, not work to redo here.
