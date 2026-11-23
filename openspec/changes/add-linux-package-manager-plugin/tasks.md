# Tasks

## Parallel execution guide

After the provider record shape and plugin identity are agreed, scanner parsing/fixtures, host registration/inventory, and Linux packaging/docs can proceed in separate files. Keep the workspace manifest/lockfile and shared package-verifier tests under one owner; integrate all streams before running workspace and package checks. Do not claim native PackageKit or pacman acceptance from fixtures alone.

## 1. Linux package discovery plugin

- [x] 1.1 Add a Linux-only `gamenode-plugin-packagekit` workspace crate and extract bounded desktop-entry parsing, executable resolution, package identity normalization, and Arch-family selection; verify fixtures cover visible Game entries, hidden/non-game entries, malformed values, duplicate packages, Arch derivatives, and other distributions.
- [x] 1.2 Implement PackageKit installed-file queries and pacman `-Qqo` queries behind the selected backend, with timeouts and typed failures; verify fixtures distinguish a successful empty scan from unavailable or failed backends and preserve safe argument-based process invocation.
- [x] 1.3 Implement the plugin with `gamenode-plugin-sdk::ProviderPlugin` for discovery only and return normalized records through `OperationResult.discovery`; verify the reference host accepts the result under `library.discovery` and rejects unsupported operations without using `PluginHost.SubmitDiscovery`.

## 2. Trusted provider integration and inventory

- [x] 2.1 Register `packagekit` as a Linux-only official descriptor with `provider.context.read` and `library.discovery`, the adjacent executable identity, and files/process/D-Bus native-access labels; embed a Linux-only provider manifest with no account, catalog, playtime, launch, or install operations; verify inventory and backend-routing tests include it only on Linux.
- [x] 2.2 Extend native-access labels and the selectable Plugins inventory for the package provider, preserving the distinction between API grants and OS access; verify English and pt-BR UI tests show the official origin, executable/session status, `provider.context.read`, `library.discovery`, and native access, and Windows inventory omits it.

## 3. Platform-specific build and package delivery

- [x] 3.1 Update Linux development and release launchers to build and stage `gamenode-plugin-packagekit` beside the host; verify the launcher fails clearly when the executable is missing and the Linux package verifier requires a valid executable x86-64 ELF.
- [x] 3.2 Keep Windows development and release commands free of the Linux-only crate and reject any packagekit executable in Windows package verification; verify Windows package fixtures and the Windows cross-build/package validation pass.
- [x] 3.3 Update plugin-build and development documentation with the Linux package layout, supported PackageKit/pacman selection, read-only discovery, `library.discovery` grant, native access, and the fact that the plugin is not sandboxed; verify every documented command and binary name matches the scripts.

## 4. Integrated candidate

- [x] 4.1 Run formatting, locked workspace build/tests, strict Clippy, plugin boundary checks, UI inventory tests/build, and OpenSpec validation; verify all complete against the candidate with Steam and PC ports regressions intact.
  - Completed (2026-11-22, branch `mvp-validation-cleanup`, integration `cc667cb`): cleared the pre-existing validation debt without runtime changes. Clippy fixes: removed an unused `mut` in `crates/core/src/library/suggest/tests.rs`; boxed `WriteOutcome::Stored` and updated its callers in `crates/platform/src/plugin_shared_data.rs`; removed a nested `format!` in `crates/platform/src/db.rs`; annotated the existing structural lints (`too_many_arguments` on `migration_preview`/`migrate_namespace`, `apply_bundle_update`, `rollback_bundle_version`, `resolve_declarations`, the runtime creation helper and test fixtures; `large_enum_variant` on `Place`); and applied the Clippy suggestions in app tests (borrows, casts, `to_string`, boolean assertions, item order). The boundary checker now passes with the stale Ludusavi allowlist line corrected and reviewed presentation/security exceptions documented (Steam achievements page, UI brand icons, reserved official provider ids, and the PackageKit-only runtime picker inventory). Validation with `CARGO_NET_OFFLINE=true CARGO_INCREMENTAL=0`: `cargo fmt --all -- --check`, `git diff --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `python3 scripts/check-plugin-boundaries.py`, `cargo build --workspace --locked` and `cargo test --workspace --locked -- --test-threads=1` passed (2756 passed, 0 failed, 4 ignored; includes 1086 app and 606 platform tests); `cargo test --locked -p gamenode-app --lib engine::tests::pc_ports` passed 21 and `engine::tests::library` passed 50; `pnpm --dir ui test --run` passed 1347 tests in 136 files; `pnpm --dir ui build` passed; `python3 -m unittest scripts.tests.test_plugin_boundaries -v` passed 16; `openspec validate add-linux-package-manager-plugin --strict` passed. No remaining validation work for this task; native PackageKit acceptance remains the workflow follow-up.
- [x] 4.2 Build the versioned Linux package and run the package verifier on the staged archive contents; verify build manifest hashes cover the packagekit binary and the Windows package verifier rejects package-manager executables.

## Workflow follow-up

- After the Linux package is available, run it on the user's Linux system and confirm that the installed SuperTux package is detected through pacman and that a failed package backend leaves prior results intact.
