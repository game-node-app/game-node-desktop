# Design

## Context

See `proposal.md` for motivation and `specs/desktop-plugins/spec.md` for behavior. The Plugin API is currently hosted by `gamenode-app`, but its allowlist, executable path, launch method, and scan entry point are Steam-specific. The external Steam executable links directly to `gamenode-core` and `gamenode-platform`. The protocol already transports generic records, while Core's provider composition remains compile-time and provider result validation requires an install directory.

PackageKit is optional on Linux, and its backend metadata is not guaranteed to classify every game consistently. This environment is Arch Linux with SuperTux installed by pacman, so the package-manager plugin should query pacman directly here and retain PackageKit for other distributions.

## Goals / Non-Goals

**Goals:**

- Make plugin selection, capabilities, execution, and typed completion provider-driven in the Tauri host.
- Give scanner executables a reusable contract independent of internal application crates.
- Discover Linux game candidates installed as packages using the distro-native query backend and installed desktop application metadata.
- Preserve Engine ownership of provider reconciliation, candidate matching, user confirmation, and persistence.

**Non-Goals:**

- Windows plugin transport, arbitrary community plugin installation, sandboxing, marketplace, or plugins writing directly to Core storage.
- Treating every installed package as a game or guaranteeing identical PackageKit metadata across distributions.
- Replacing the existing Tauri WebView IPC.

## Decisions

### Use a host-owned discovery registry

Replace `begin_steam_discovery` and `run_steam_discovery` with provider-keyed descriptors containing plugin identity, executable, and the exact discovery capability. The Engine's scan effect asks the manager to run the descriptor associated with the provider. The host binds each session to the provider and operation and rejects mismatched records. Keep the registry packaged and host-owned; do not allow plugins to register themselves or request broader permissions.

Discovery plugins share the `library.discovery` capability. The host still binds each session to one registered provider and scan operation, and rejects mismatched results. Plugins can submit only records and terminal status; they do not receive GameNode credentials, write persistence, or call arbitrary Engine commands.

### Separate scanner code from its host adapter

Extract the scanner contract and Steam implementation into a shared scanner crate with no dependency on `gamenode-core` or `gamenode-platform`. The in-process Steam provider and external Steam executable both adapt this shared implementation to their respective file access and Plugin API boundaries. The Linux package manager executable depends only on the shared plugin protocol and its native package manager adapters, returning the same typed discovery record shape.

Keep provider catalogue metadata, launch templates, provider reconciliation, and storage in the host/Core. Plugin output is untrusted input: validate provider identity, operation, ids, display names, paths, and record limits at the host boundary before passing it into Engine reconciliation.

### Select the distro package backend and classify candidates conservatively

The Linux package-manager plugin queries installed package ownership asynchronously. On Arch-based distributions it uses pacman's native query interface, avoiding PackageKit's incomplete ALPM behavior. On other distributions it uses the PackageKit system service. It classifies a package as a game when an installed desktop entry belonging to that package declares a game category. Use package identity as the provider-local id, desktop display name and launch metadata where present, and an available executable/application location for the install location required by the current record contract. Deduplicate package and desktop-entry observations.

Do not infer that all packages with suggestive names are games. Ignore packages without desktop game metadata. A missing selected package manager or permission is a normal provider failure, so other providers continue and previous results remain intact. Keep package-manager calls and local metadata inspection off the UI thread, with bounded operation time and result counts.

### Keep Engine behavior and UI contracts stable

Route each successful plugin completion through the existing `ProviderScanned` transition. Existing match review and persistence remain authoritative. A Linux package manager result is a source of installed-game candidates, not a claim that the candidate has been matched to a GameNode catalogue entry. Keep `invoke`, `engine:snapshot`, and the scan coordinator contracts unchanged.

### Retain platform-specific transport boundaries

Linux remains on the authenticated private UDS transport. Windows continues using its existing in-process Steam scanner until a separate named-pipe transport is implemented and validated. Linux package discovery is Linux-only; its absence does not disable the desktop or other scanners.

## Risks / Trade-offs

- [PackageKit backends may omit or misclassify installed file ownership] → Use pacman's native query interface on Arch-based distributions and PackageKit elsewhere; preserve prior data on backend failures.
- [Desktop launch metadata may not identify one package directory] → Preserve package identity and available desktop launch data; avoid fabricating an install path and reject records that cannot satisfy the host's current record contract.
- [A generic host registry expands the trusted input surface] → Keep descriptors compiled/package-owned, capabilities provider-scoped, and validate all returned fields and limits.
- [Extracting Steam scanner behavior can cause platform drift] → Keep the in-process and plugin adapters on one shared scanner implementation and verify equivalent record ids, names, and directories.

## Migration Plan

1. Generalize host descriptors, capability grants, process launch, completion routing, and tests while preserving Steam behavior.
2. Extract reusable scanner types/Steam logic and migrate both current Steam paths; verify plugin dependencies do not include `gamenode-core` or `gamenode-platform`.
3. Add the Linux package-manager scanner executable and host registration; verify distro selection and simulated installed-game, non-game, empty, malformed, and unavailable-backend responses.
4. Package both official Linux plugins, document authoring and permissions, then run the existing Steam acceptance and a live SuperTux/pacman acceptance plus PackageKit acceptance on a supported non-Arch host.

Rollback can disable the Linux package manager descriptor and restore the prior Steam-specific host routing without changing the database schema. The Engine remains the only profile owner throughout.

## Open Questions

None. Live PackageKit acceptance remains environment-dependent and can be completed on a supported non-Arch host; this Arch environment can validate the native pacman path.
