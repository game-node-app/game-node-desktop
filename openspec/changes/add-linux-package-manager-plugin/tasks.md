# Tasks

## Parallel execution guide

After the provider record shape and plugin identity are agreed, scanner parsing/fixtures, host registration/inventory, and Linux packaging/docs can proceed in separate files. Keep the workspace manifest/lockfile and shared package-verifier tests under one owner; integrate all streams before running workspace and package checks. Do not claim native PackageKit or pacman acceptance from fixtures alone.

## 1. Linux package discovery plugin

- [x] 1.1 Add a Linux-only `gamenode-plugin-packagekit` workspace crate and extract bounded desktop-entry parsing, executable resolution, package identity normalization, and Arch-family selection; verify fixtures cover visible Game entries, hidden/non-game entries, malformed values, duplicate packages, Arch derivatives, and other distributions.
- [ ] 1.2 Implement PackageKit installed-file queries and pacman `-Qqo` queries behind the selected backend, with timeouts and typed failures; verify fixtures distinguish a successful empty scan from unavailable or failed backends and preserve safe argument-based process invocation.
- [ ] 1.3 Implement the plugin with `gamenode-plugin-sdk::ProviderPlugin` for discovery only and return normalized records through `OperationResult.discovery`; verify the reference host accepts the result under `library.discovery` and rejects unsupported operations without using `PluginHost.SubmitDiscovery`.

## 2. Trusted provider integration and inventory

- [x] 2.1 Register `packagekit` as a Linux-only official descriptor with `provider.context.read` and `library.discovery`, the adjacent executable identity, and files/process/D-Bus native-access labels; embed a Linux-only provider manifest with no account, catalog, playtime, launch, or install operations; verify inventory and backend-routing tests include it only on Linux.
- [x] 2.2 Extend native-access labels and the selectable Plugins inventory for the package provider, preserving the distinction between API grants and OS access; verify English and pt-BR UI tests show the official origin, executable/session status, `provider.context.read`, `library.discovery`, and native access, and Windows inventory omits it.

## 3. Platform-specific build and package delivery

- [x] 3.1 Update Linux development and release launchers to build and stage `gamenode-plugin-packagekit` beside the host; verify the launcher fails clearly when the executable is missing and the Linux package verifier requires a valid executable x86-64 ELF.
- [ ] 3.2 Keep Windows development and release commands free of the Linux-only crate and reject any packagekit executable in Windows package verification; verify Windows package fixtures and the Windows cross-build/package validation pass.
- [x] 3.3 Update plugin-build and development documentation with the Linux package layout, supported PackageKit/pacman selection, read-only discovery, `library.discovery` grant, native access, and the fact that the plugin is not sandboxed; verify every documented command and binary name matches the scripts.

## 4. Integrated candidate

- [ ] 4.1 Run formatting, locked workspace build/tests, strict Clippy, plugin boundary checks, UI inventory tests/build, and OpenSpec validation; verify all complete against the candidate with Steam and PC ports regressions intact.
- [x] 4.2 Build the versioned Linux package and run the package verifier on the staged archive contents; verify build manifest hashes cover the packagekit binary and the Windows package verifier rejects package-manager executables.

## Workflow follow-up

- After the Linux package is available, run it on the user's Linux system and confirm that the installed SuperTux package is detected through pacman and that a failed package backend leaves prior results intact.
