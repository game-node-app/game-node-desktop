# Steam plugin integration evidence

## Baseline

Integration branch: `integration/steam-plugin-on-main`.

| Source | Revision | Role |
| --- | --- | --- |
| `origin/main` | `578ffa72687847f95b781d6862a7fdb1dc05e390` | Application baseline with PC ports and schema 8 |
| `integration/migrate-steam-to-plugin` | `f19f9323473a95d70521571a274937f32f309489` | Donor planning state; Steam implementation includes `77389ce1fb3a4e218ab4fbe3504b115883ea09de` |
| Common ancestor | `1cbac996a22d1cae1ecd1f26fe407cf407b0d007` | Ancestry reference |

The integration branch descends from the selected mainline SHA. The donor branch remains available unchanged. The current integration head begins at `c6ff8f9d68f656413b93c38f6eefc58e4820cf16`, which adds the approved OpenSpec plan to that baseline.

## Parity matrix

Evidence columns are intentionally empty until run against the integrated candidate. Donor test results are historical reference only. Native Windows acceptance follows recorded user confirmation of Linux acceptance.

| Area | Mainline behavior to preserve | Linux candidate | Windows VM candidate |
| --- | --- | --- | --- |
| Installed discovery | Native and Flatpak roots, all libraries, exclusions, deduplication; Windows registry and drives | Pending | Pending |
| Login and Guard | Isolated login, authenticated-page verification, focus/cancel/close/timeout | Pending | Pending |
| Session and secrets | Restore, cookie rotation, expiry/unreachable distinction, keyring failures, Linux private-file fallback, Windows Credential Manager | Pending | Pending |
| Owned and Family catalog | Full uninstalled/unplayed/free library, independent category results, retries, account switch and sign-out | Pending | Pending |
| Library and GUI | Stable `steam:<appid>`, installed/owned/Family facts, matches, favorites, covers, filters, selection and cached banners | Pending | Pending |
| Play and Install | Existing Steam URI actions, missing-handler result and cached declarations | Pending | Pending |
| Store and Internal playtime | Counter conversion/latest local account; Windows process tracking, attribution, triggers and deduplication | Linux tracking remains unavailable as in mainline | Pending |
| Runtime and lifecycle | Missing plugin, crash/restart, scoped permissions, stale results, responsiveness and shutdown cleanup | Pending | Pending |
| PC ports regression | Mainline catalog, overlay, guided/install flow, migration 0008, provenance, cancellation/recovery, launch environment and gameplay roles | UI/mock and applicable Core/platform checks pending; installation is Windows-only | Pending native installation and lifecycle acceptance |
| Other mainline features | GameNode auth, manual games, settings, backup/game tools, tray/lifecycle | Pending applicable flows | Pending applicable flows |
| Packaging and boundaries | Adjacent executable, vendored Protobuf compiler, trusted grants, no PATH fallback or internal plugin dependencies | Pending | Cross-build is not native installer execution; VM pending |

## Donor task mapping

The donor reported 63/72 tasks complete, with nine remaining. Those checkboxes and historical evidence do not transfer as acceptance for this mainline-based candidate.

| Donor task | Obligation carried here | Evidence status |
| --- | --- | --- |
| 3.3 | Native Windows named-pipe tests and compilation | Pending integrated candidate and VM |
| 10.1-10.4 | Linux real login/profile, Steam workflows, responsiveness/cleanup, then explicit user confirmation | Pending |
| 11.1-11.4 | Windows native transport/runtime, Steam workflows, gameplay counters and final parity report | Pending after Linux confirmation |

Earlier donor task 3.3 also includes transport behavior that had partial Windows cross-target evidence. Cross-target compilation remains distinct from native pipe execution.

## PC ports obligations retained from mainline

Mainline `add-pc-port-catalog` records these pending tasks at the selected baseline; the integration keeps them visible and validates them on the integrated candidate:

- 1.2: verify the selected extractor against an owned base/update package and Lantern Engine.
- 5.4: implement/verify extractor execution, cleanup and cancellation behavior.
- 10.1: Windows acceptance for release-only, guided and package installs, cancellation/Quit, removal and launch/playtime behavior.
- 10.2: locked app/Windows/UI checks and strict OpenSpec validation.

No Linux port-install support is claimed. Windows ports acceptance belongs in the user-provided VM after Linux Steam confirmation.

## Candidate evidence log

The implementation checkpoint is `99e633f` (`feat(plugins): host Steam provider in app`), on top of `c6ff8f9d68f656413b93c38f6eefc58e4820cf16`; follow-up verification and integration work continues in the working tree. The following fixture and cross-build evidence was produced on 2026-11-08:

| Area | Command | Result | Limits |
| --- | --- | --- | --- |
| Public contract and reference host | `cargo test --locked -p gamenode-plugin-protocol -p gamenode-plugin-sdk --all-targets` | 22 tests passed | Protocol/SDK fixtures only; no application integration implied. |
| Steam implementation | `cargo test --locked -p gamenode-plugin-steam --all-targets` | 108 tests passed | Scanner, HTTP, account, login and counter fixtures; no real Steam session. |
| Profile upgrades and schema 8 preservation | `cargo test --locked -p gamenode-platform db::tests::` | 73 tests passed | Includes v6→v8 Steam preservation, existing v7→v8, and reopening populated schema-8 Steam/PC Ports rows; no migration changes. |
| Host transport, brokers, credential adapter, process supervision and login lifecycle | `cargo test --locked -p gamenode-app --lib plugin_host::` | 40 tests passed | Includes Linux socket owner/mode/refusal, process crash/restart and login profile fixtures; no native Windows named-pipe or live WebView acceptance. |
| Windows target compilation | `cargo xwin check --locked -p gamenode-app --bin gamenode --target x86_64-pc-windows-msvc` | Passed | Cross-compilation only; no native named-pipe DACL, client-rejection or Windows UI acceptance. |
| Credential behavior | `cargo test --locked -p gamenode-platform credentials::`; `cargo test --locked -p gamenode-app --lib an_unreachable_steam_keeps_the_credential_and_the_cached_library` | 16 platform credential tests and 1 cache-retention Engine test passed | Linux fixtures; Windows no-plaintext behavior was source-reviewed and cross-compiled, not exercised in a Windows VM. |
| Plugin dependency boundaries | `python3 scripts/check-plugin-boundaries.py`; `python3 -m unittest scripts/tests/test_plugin_boundaries.py -v` | Checker passed; 6 boundary tests passed | Static dependency graph/fixtures; CI execution remains part of integrated checks. |
| Development launch workflows | `python3 -m unittest discover -s scripts/tests -p test_desktop_launcher.py -v`; `bash -n scripts/run-desktop.sh scripts/dev-windows-wsl.sh scripts/build-windows-wsl.sh` | 2 launcher tests passed; Bash syntax passed | Verifies locked host+plugin build intent, adjacency checks and no launch when the plugin is absent; no native Windows launch. |
| Release package layout | `python3 -m unittest scripts/tests/test_plugin_package.py`; `bash -n scripts/package-linux.sh scripts/package-windows-wsl.sh scripts/build-windows-wsl.sh`; `python3 -m py_compile scripts/verify-plugin-package.py scripts/tests/test_plugin_package.py` | 5 package fixtures passed; shell syntax and Python compilation passed | Validates host/plugin architecture, missing/wrong-platform binaries, paths with spaces, licenses and existing resources; archives still require package acceptance in later sections. |
| Host compilation | `cargo check -p gamenode-app` | Passed after adapting imported modules to mainline Tauri/Core APIs | Does not exercise Engine or Tauri application startup; additional integration is pending. |
| Engine/Core and PC Ports regression fixtures | `cargo test --locked -p gamenode-app --lib engine:: -- --test-threads=1` | 155 tests passed | Fixture-based Engine/Core behavior, including account/catalog transitions, playtime and PC Ports; includes failing Steam runtime with preserved PC ports job; not packaged application acceptance. |
| Steam action and playtime contract fixtures | `cargo test --locked -p gamenode-core library::launch::`; `cargo test --locked -p gamenode-core playtime::submitter::`; `cargo test --locked -p gamenode-plugin-steam discovery::counter::` | 7 launch/URI, 4 submission and 2 Steam-counter tests passed | Fixtures cover missing URI handlers, template encoding, accepted-total deduplication, latest Steam user selection and local counter parsing; no native Steam client launch. |
| Plugin account and inventory UI | `cargo check --locked -p gamenode-app --bin gamenode`; `ui/node_modules/.bin/vitest run src/features/settings/StoresPanel.test.tsx src/features/settings/PluginInventory.test.tsx src/features/settings/SteamAccountCard.test.tsx`; `ui/node_modules/.bin/tsc -b` | App check passed; 23 UI tests passed; TypeScript build passed | Covers generic provider account actions/progress/failures, trusted official origin, executable-missing state, permissions/native access, and no credential material in inventory DTO; English and pt-BR labels. |
| Mainline UI and combined Steam/PC ports fixtures | `ui/node_modules/.bin/vitest run src/mock/host.test.ts src/features/shell/shell.test.tsx src/features/ports/ports.test.tsx src/features/ports/ports.ptBR.test.tsx src/features/ports/setup/setup.test.tsx src/features/settings/SettingsModal.test.tsx src/features/settings/StoresPanel.test.tsx src/features/settings/PluginInventory.test.tsx` | 116 tests passed across 8 files | Covers shared mock commands/inventory and Steam+ports snapshot in English/pt-BR; shell, ports, guided setup, settings/cache and plugin inventory. |
| Complete UI suite and production bundle | `ui/node_modules/.bin/vitest run --maxWorkers=1 --minWorkers=1`; `ui/node_modules/.bin/tsc -b`; `ui/node_modules/.bin/vite build` | 301 tests passed across 28 files; TypeScript and production build passed | Serial suite avoids environment-level cross-file contention. Build regenerates tracked `ui/dist`; Vite reports the existing >500 kB main chunk warning. Focus, modal, virtualization and artwork suites passed. |

Separate fixture/cross-build evidence from real Linux/Windows execution. Add candidate/package identities and profile details as work proceeds. Leave any scenario without evidence marked pending or unresolved.
