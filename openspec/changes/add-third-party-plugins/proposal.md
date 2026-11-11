# Proposal

## Why

GameNode already has a local gRPC provider protocol and Rust SDK, but only trusted bundled executables can use them. Third-party developers need a documented, independently implementable contract and users need explicit control over installation, host permissions, execution and updates.

## What Changes

- Add a separate, managed third-party plugin registry on Windows and Linux, with installation, activation, disabling, removal and failure isolation. Existing official plugins, especially Steam, retain their descriptors, behavior, credentials and packaging.
- Define a versioned package manifest, per-file integrity checks, publisher signatures and identity pinning, platform selection, signed update metadata, compatibility checks and atomic update/rollback.
- Declare typed per-target OS/architecture, Windows version/build/product and Linux libc/distribution requirements separately from author-tested environments. Detect the effective execution environment in Core/platform before plugin code runs, block incompatible or indeterminate required checks and show actionable compatibility reasons.
- Add persistent, independently revocable API permissions, scoped resources, explicit machine-readable denials and permission-change notifications. Runtime, library-root and save-profile read/write/delete are independent, initially creator-scoped after consent, with explicit category-wide or selected-record operation grants for other plugins' published records; private namespaces remain isolated. Native processes remain unsandboxed; distinguish enforced API grants from declared native access and explain that GameNode cannot confine direct OS access.
- Publish a finite initial resource/permission catalog and implement its host RPCs for library, metadata, favorites, artwork, launching, scoped storage/network/files, notifications, backups, tools and PC ports. Preserve domain validation and confirmations for actions that affect files or execute software.
- Add `game.launch` for unchanged detected URI/executable targets and `game.custom_launch` for approved alternate executables, arguments, working directories, runtime wrappers and environment overrides. Launch extensions can target user-approved games from any provider, including official providers, without modifying those providers.
- Expose multiple-library/root registration and one GameNode game identity with separate ROM/content resources and selectable runtime bindings. Runtime plugins may request scoped discovery; switching emulators preserves game history, content and save references.
- Record optional save format/naming/runtime-compatibility descriptors and plugin-defined save locations independent of ROM folders. Reserve save selection/import/conversion, screenshots, last-loaded events, platform slots and shared-container browsing for future negotiated capabilities; no Core emulator parsers or converters are required now.
- Add save-profile configuration and common location inspection for saves/installations/runtimes: effective access, size, dates, hashes/comparison, mount availability and storage characteristics. Accept plugin-produced benchmark/SMART reports without executing diagnostic tools in Core.
- Accept, retain as authorized and relay hardware, in-game presence and game-performance observations (FPS, CPU, RAM). API data ingestion/query/subscriptions ship independently of future Core dashboards, predictions or public-profile integration.
- Require producer-requested, user-authorized publication to a local public pool distinct from private storage. Let users independently select publication/read/create/update/delete permissions at installation or in settings, with category-wide or selected-record scopes and separate persistence consent. Let users select common-data category read permissions during plugin activation. A grant permits, but does not trigger, access to published records from any plugin in that category (including game paths, performance history and same-console ROM availability); the plugin explicitly chooses per query whether to include common data. Keep explicit source/recipient field-level review for sensitive/custom data, exclude secrets, and keep persistence/retention separate.
- Classify every RPC field: typed control/action/security contracts, typed resource/report bases with namespaced versioned Struct extensions, and typed envelopes with opaque bytes for files/private blobs. Publish an endpoint-by-endpoint route/field/extensibility matrix; extensions never grant authority or replace validated action fields.
- Add independent `storage.persist` consent, plugin-attributed disk usage, uninstall retain/delete choices and API-only own-namespace cleanup/retention policies without exposing physical paths to plugins. Persistent game-performance history can be reused by current or future plugins that hold the category read grant and explicitly include common data in each query.
- Require a host-generated launch-plan review for every GameNode-mediated launch customization, including runtimes/emulators, mod layers and custom targets/arguments/environment. Bind one-time or remembered approval to the exact plan, show provenance and effective changes, and reject prohibited requests before confirmation without claiming executable safety.
- Establish API negotiation, support/deprecation rules and capability-based fallback for third-party clients, without retroactively applying third-party compatibility policy to official plugins.
- Prepare independently consumable MIT SDK/protocol/example distributions, conditional on documented copyright authority; keep the core and host GPL-3.0-only. SDK use is optional.
- Deliver two practical placeholder plugins: Go using generated gRPC bindings directly without the GameNode SDK, and Rust using the SDK. Cover real requests, responses, streams, standard/custom launch recipes, denial and compatibility handling against the same host.
- Let plugins own their functional configuration window, opened or focused through Configure in GameNode. Keep declarative forms optional; installation, permissions, activation, updates and licenses stay in GameNode.
- Extend settings with third-party installation, permissions, effective capabilities, native-access warnings, version/update status and offline license notices. Validate declared license metadata and distribution boundaries without claiming automatic legal certification.

## Capabilities

### New Capabilities

- `desktop-external-plugins`: Managed third-party installation, publisher identity, integrity, lifecycle, updates, persistent-data accounting and cleanup.
- `desktop-plugin-permissions`: Persistent scoped grants, common-data category permissions, sensitive/custom field-level sharing and retention consent, explicit denials, revocation and accurate disclosure of native access.
- `desktop-plugin-api`: Language-neutral data/resource contracts, inter-plugin exchange, save/storage inspection, libraries/runtimes, performance/presence, game launch and version compatibility.
- `desktop-plugin-development`: MIT public development assets, documentation and Rust/Go interoperability examples.

### Modified Capabilities

- `desktop-playtime`: Add runtime content-session evidence, manifest-declared playtime/pause capabilities, disclosed optional application-open accounting and a user disable control while preserving official source/platform policies.
- `desktop-plugin-services`: Distinguish private isolation from authorized public-pool access and accept provider or scoped runtime-content discovery operations.

- `desktop-providers`: Permit validated third-party provider registrations and local executable targets, plus independent launch extensions, while preserving bundled manifest trust and reserved identities.
- `desktop-settings`: Add third-party management, Configure entry point and license/permission disclosure to the existing plugin inventory in Stores.

## Impact

Shared protocol and SDK additions; new third-party host/registry/package/update services beside `crates/app/src/plugin_host*`; domain adapters in app/core; persistence and OS adapters in platform; async Tauri commands; existing React `PluginInventory`, query bridge, mock and translations; public developer docs, examples and packaging tooling. Proposed dependencies include archive/hash/signature/SPDX tooling and Go gRPC plus a Windows named-pipe adapter for the standalone example.

This change plans no official plugin migration, marketplace, arbitrary plugin JavaScript in the UI, kernel/OS sandbox or direct database/Core credential access. It evolves the Steam integration baseline consolidated in the main `desktop-plugins`, `desktop-plugin-services` and `desktop-providers` specs. Their official behavior and API 1.1–1.3 contracts remain compatibility constraints. The managed third-party route supersedes the bundled-only restriction for separate approved identities without weakening official registration or backend validation. The Linux package-manager change still overlaps shared host code; its existing official behavior is also preserved. Synchronizing the Steam specs does not complete its pending native acceptance or authorize repeating that migration here.
