# Design

## Context

The current release registers only Steam in `crates/app/src/plugin_host.rs`; the Linux package scanner is absent from the workspace. Its previous implementation is recoverable from commit `3cf67ab`, but it connects through `PluginHost.SubmitDiscovery`, which the current host deliberately returns as `UNIMPLEMENTED`. The supported path is `gamenode-plugin-sdk::ProviderPlugin` over `ProviderHost.Exchange`. The existing inventory already reads trusted host descriptors, so registering the Linux provider makes it visible without a package-specific UI branch.

Linux packaging and development launchers currently build and stage only the host and Steam executable. The package verifier explicitly rejects the package-manager binary on Linux, while Windows scripts build only Steam. The existing provider registry loads manifests only for their declared platform and derives plugin backends from trusted descriptors.

## Goals / Non-Goals

**Goals:**

- Reuse the old PackageKit/pacman discovery behavior while moving its operations onto the current public SDK.
- Make the Linux package provider a trusted, official, discovery-only provider using `library.discovery`.
- Build, verify, and stage the adjacent package-provider executable for Linux development and release delivery.
- Keep Windows build and package requirements independent of the Linux-only executable.
- Ensure users can inspect this plugin in the existing selectable inventory and see API grants separately from its native OS access.

**Non-Goals:**

- Add a second plugin protocol, account or catalog operations, secrets, login UI, actions, or plugin settings.
- Add package-manager support to Windows or change Steam and PC ports behavior.
- Promise a sandbox: native file, process, and D-Bus access remains governed by the OS and is displayed separately from API capabilities.
- Install, remove, or modify system packages. Discovery is read-only.

## Decisions

### Use the current SDK event stream

Port the scanner implementation into a Linux-only `gamenode-plugin-packagekit` crate implementing `ProviderPlugin`. It handles only `Discover`, obtains the provider context from the host, and returns records through the standard `OperationResult.discovery` event. Do not call `PluginHost.SubmitDiscovery` or change Protobuf/API versions. This keeps the same public contract used by Steam and avoids reviving the disabled compatibility RPC.

**Alternative:** re-enable `SubmitDiscovery` for legacy clients. Rejected because it would create a second host execution path with separate validation and lifecycle behavior.

### Keep package discovery read-only and provider-local

Retain the prior candidate strategy: inspect bounded system desktop-entry directories, consider visible Application entries in the Game category, resolve an existing executable, and query which installed package owns that entry. Use the normalized package name as the provider-local identity and return the executable path and package-manager source as fields. Do not derive executable actions from the record; the provider manifest declares no launch or install URI.

**Alternative:** expose every installed package or launch arbitrary `Exec` contents. Rejected because the goal is game candidates and the host must not turn package metadata into executable authority.

### Select the backend from the distribution family

Read `/etc/os-release` on a blocking worker. Use `pacman -Qqo` for Arch and Arch-family systems, and PackageKit's installed-file query for other distributions. Keep PackageKit D-Bus operations asynchronous and bounded, and keep the pacman process invocation argument-based with a fixed executable name and locale. A selected backend failure fails only this provider scan; successful empty results remain distinguishable from failures and the Engine preserves the previous successful provider records.

**Alternative:** always prefer PackageKit and fall back after a query error. Rejected because PackageKit is known to be unreliable with pacman; choosing pacman for Arch avoids turning that limitation into repeated scan failures.

### Register and describe the plugin through trusted host data

Add a Linux-only descriptor with provider id `packagekit`, plugin id `org.gamenode.plugin.packagekit-discovery`, display name `Gerenciador de pacotes Linux`, only the `provider.context.read` and `library.discovery` grants, and executable name `gamenode-plugin-packagekit`. Context read is required by the SDK to receive the trusted provider manifest and platform. Embed a Linux-only provider manifest with no account, catalog, playtime, launch, or install declarations. Report native access as files, process execution, and system D-Bus; do not represent these OS facilities as API permissions or as a sandbox boundary.

**Alternative:** discover arbitrary executables from disk or `PATH`. Rejected because plugin identity and grants must remain host-owned, and packages must run the exact adjacent binary.

### Package the executable only on Linux

Update the Linux run/package scripts to build and place the plugin beside the host, make the Linux verifier require a valid executable x86-64 ELF for it, and update release metadata/docs. Keep Windows launch/build/package scripts unchanged except for tests proving that packagekit remains absent; the host descriptor and manifest must filter it out on Windows.

**Alternative:** make the plugin an unconditional application dependency. Rejected because that would unnecessarily build or ship Linux package-manager code for Windows and violate the platform boundary.

## Risks / Trade-offs

- [PackageKit D-Bus signatures or distro behavior differ] → Keep D-Bus query and signal parsing behind focused fixtures, enforce time/message/record limits, and report a provider failure without clearing prior results.
- [Package names or desktop files produce duplicate candidates] → Normalize provider-local package identity and deduplicate records before submitting them.
- [A malicious or malformed desktop `Exec` value influences execution] → Treat it only as a candidate path; never execute the desktop command or declare it as a GameNode action.
- [Linux-only dependencies break Windows cross builds] → Put D-Bus dependencies and scanner code behind Linux target configuration, and verify Windows cross-build/package checks.
- [Plugin binary is missing from a development or release directory] → Make launchers/package verification fail with a specific missing-binary message before starting the host.

## Migration Plan

1. Add the Linux-only plugin crate, provider manifest, and trusted host descriptor; adapt the recovered scanner to the SDK and shared discovery capability.
2. Update Linux development and release build/staging scripts, checksums/build manifests, package verifier fixtures, and plugin documentation. Keep Windows output unchanged.
3. Run scanner fixtures, host/provider routing checks, Linux package-layout checks, UI inventory checks, strict formatting/lint/build checks, and the Windows cross-build/package verification available in this environment.
4. Build the Linux package and let the user validate package discovery on their Linux system, including SuperTux via pacman. Do not claim live backend acceptance from fixtures or package-layout validation.

Rollback removes the Linux packagekit descriptor, manifest and executable from the build/package scripts. Existing persisted library records remain under the provider id `packagekit`; the Engine's normal failed-provider handling preserves prior successful records if the executable is unavailable.
