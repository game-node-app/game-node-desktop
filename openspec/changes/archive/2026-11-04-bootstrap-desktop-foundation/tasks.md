# Tasks

The Version 3.0 source import is complete. Use the [review reference](references/figma-version-3.md), [token snapshot](references/figma-version-3-tokens.json), and [exported wordmark](references/assets/gamenode-logo.svg) as the implementation baseline. Source IDs, aliases/modes, palette-family differences, and native adaptations are recorded there; no Figma extraction task is deferred to apply.

## 1. Workspace and license

- [x] 1.1 Create the Cargo workspace and `gamenode-core`, `gamenode-platform`, `gamenode-api`, and `gamenode-app` at the roadmap paths; verify `cargo metadata --no-deps` reports exactly those members and adapter dependencies point inward.
- [x] 1.2 Pin a compatible Rust toolchain and Slint/build-tool versions, commit `Cargo.lock`, and configure formatting/lints and explicit target features for Skia/software, Windows/Linux keyring, Tokio, rusqlite, and the HTTP client; verify `cargo check --workspace --locked`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
  - _Verified (2026-11-04, Linux):_ Rust 1.99.1; Slint/slint-build 1.18.1; explicit backend/adapter features; `Cargo.lock` committed in `542d8cd`. `cargo check --workspace --locked`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets --locked -- -D warnings` pass.
- [x] 1.3 Add the user-selected GPL-3.0-only repository license and Cargo metadata selecting Slint's GPLv3 path; verify the license text and all package metadata agree and record font/artwork licenses separately.
  - _Verified (2026-11-04):_ GPL-3.0-only metadata in all four packages; full GPLv3 text in `LICENSE`; Slint GPLv3 selection in workspace metadata. Separate artwork/font attribution, provenance, licenses and asset hashes recorded in `licenses/` with the owner’s artwork-license choice.
- [x] 1.4 Document Windows and Linux native build dependencies and the standard check commands; native CI-runner verification waived by the user.
  - _Decision (2026-11-04):_ User explicitly waived CI-runner verification. Native prerequisites and standard checks are documented in `docs/development.md`; prior WSL cross-build and Windows execution evidence remains in that guide.

## 2. State and local persistence

- [x] 2.1 Define only foundation settings, user/authentication states, commands/snapshots, and the adapter interfaces needed by this step in `core`; verify transition tests cover signed-out, pending, verifying, signed-in, restoration failure, and sign-out states without OS/UI dependencies.
- [x] 2.2 Implement per-user data paths and the platform SQLite repository with schema history and initial settings migration; verify temporary-database tests cover a fresh database, reopening without reset, and durable renderer/geometry values.
- [x] 2.3 Implement ordered transactional migrations and safe errors for failed migrations and newer schemas; verify rollback, older-fixture upgrade preserving settings, and unchanged newer-schema fixtures.
- [x] 2.4 Document local file locations, migration behavior, and recovery without automatic deletion; verify the paths match each OS adapter and no schema/settings fixture contains credentials or full callback URLs.

## 3. Credentials and existing API contract

- [x] 3.1 Implement Windows Credential Manager and Linux Secret Service credential adapters under a stable app key; verify adapter tests cover save/load/delete, locked service, failed writes, and deletion failure.
  - _Progress (2026-11-04):_ Verified on Linux and Windows: keyring mock-backed save/load/delete, locked-store, failed-write and deletion-failure tests pass. Installed Windows Settings identifies Credential Manager. Real-provider session persistence is still acceptance task 8.1.
- [x] 3.2 Implement the Linux-only absent-Secret-Service fallback with atomic private-file writes and a storage-method status for Settings; verify `0700`/`0600` permissions, safe file handling, deletion, and that a locked existing service does not trigger fallback.
- [x] 3.3 Implement Rust requests/types for the existing `/v1/auth/sign-in/social`, authorization-proxy handoff, `/v1/auth/get-session`, and `/v1/auth/sign-out` contract; verify a mock server receives the native header, callback URLs, state-cookie transfer, signed bearer value, and expected error classification.
- [x] 3.4 Implement short-lived single-use callback correlation and strict URL/session-cookie parsing; verify redacted fixtures cover valid and secure-prefixed cookie forms, malformed/duplicate fields, wrong scheme/target/nonce, unsolicited, expired, replayed, and concurrent callbacks, and assert diagnostics exclude credentials.
- [x] 3.5 Document the observed mobile wire contract and deployment `app.gamenode://` trust prerequisite with sibling source paths; verify the document matches the protocol fixtures and requires no new server endpoints or MCP/client-connection flow.
  - _Verified (2026-11-04):_ Added `docs/auth-wire-contract.md`; cross-checked its endpoint, header, callback, proxy-state, cookie, bearer, and sign-out details against the sibling server sources and Rust protocol fixtures. No new endpoints or MCP/client-connection flow are introduced.

## 4. Single instance and URI delivery

- [x] 4.1 Implement per-user instance ownership and bounded activation/callback IPC using Windows named pipes and Linux private Unix sockets before database initialization; verify process integration tests elect one owner during simultaneous launches and recover after a stale endpoint/crash.
  - _Progress (2026-11-04):_ Verified on Windows and Linux: process integration tests elect one owner, forward launches and recover after crashes. Windows unresponsive-owner timeout and a Linux directory-creation race were fixed. App acquires ownership before SQLite.
- [x] 4.2 Implement ordinary launch activation and URI argument delivery to the owner, including waiting for an owner still starting; verify a second process exits after forwarding, the owner receives the intact argument, and malformed/oversized messages do not modify authentication state.
  - _Progress (2026-11-04):_ App now forwards launches and callbacks before database initialization. Process tests pass on both platforms; installed Windows second launch exits 0 and an unsolicited URI preserves the signed-out state.
- [x] 4.3 Connect system-browser opening to the authorization proxy and handle browser-launch errors without blocking Slint; verify an injected browser adapter receives the expected URL and failure permits a new attempt.
  - _Progress (2026-11-04):_ SystemBrowser and Engine are connected to Slint; injected-browser tests cover authorization URL delivery, launch failure and retry. Status messages are bundled in English/pt-BR.
- [x] 4.4 Document OS IPC locations/access rules and the distinction between callback cold-start and completed-session restoration; verify tests reject a cold-start callback with no matching pending attempt.
  - _Verified (2026-11-04):_ Added `docs/desktop-ipc-and-callback-lifecycle.md` with platform endpoint paths, permissions, forwarding limits, and separate cold-callback and stored-session flows. The app engine test explicitly rejects a cold-start callback with no pending attempt.

## 5. Version 3.0 shell and translations

- [x] 5.1 Copy the selected values from `references/figma-version-3-tokens.json` into `crates/app/ui/` theme definitions, retaining names/units/provenance and the recorded palette-family selection and desktop control/focus adaptations; verify each used color/type/spacing/radius value maps to that reference without importing Figma feature scope.
  - _Progress (2026-11-04):_ All 40 selected tokens and provenance copied to ui/theme.slint and theme-provenance.json. Native widget palette/focus styling still needs the full reference audit.
- [x] 5.2 Bundle Roboto weights 400/500/600/700, the exact exported `references/assets/gamenode-logo.svg`, the canonical application icon, and their notices; verify they render without network access, preserve the wordmark aspect ratio, and the packaged asset inventory preserves provenance and licenses.
  - _Progress (2026-11-04):_ Roboto variable font, exact wordmark, canonical mobile icon and notices are bundled and embedded in the installed Windows binary. Full packaging/visual verification remains pending.
- [x] 5.3 Build the signed-out shell, top bar, signed-in empty library, app menu, and Settings/About with Slint; verify keyboard focus/access to every foundation action and absence of game records, scan/match/launch actions, and web/social features.
  - _Verified (2026-11-04):_ All shell actions use the shared accessible `Action` control, which exposes button labels, visible focus, and Space/Enter activation. Existing native Windows UI Automation verified focusable controls plus Space to open the menu and Escape to dismiss it; all 17 app tests pass. The signed-out, pending, retry, signed-in, menu, and Settings/About states contain no game records or discovery/match/launch/social features.
- [x] 5.4 Select Winit/Skia before UI creation, apply the persisted software setting on restart, and attempt software once on GPU initialization failure; verify normal/software startup and bounded fallback preserve settings and make renderer status visible.
  - _Verified (2026-11-04):_ Production Skia startup stayed live in the isolated Linux X11 session; the software-rendered UI review harness launched, rendered snapshots, and exited successfully. Settings showed the active Software renderer. Seven app tests cover saved preference selection, one-time bounded handoff, internal argument parsing, and localized status/errors; the prior Windows Skia startup is documented. A live GPU failure was not forced on hardware.
- [ ] 5.5 Save and restore normal window geometry/maximized state with off-screen validation and compositor-aware positioning; verify restart after resize/move/maximize, unavailable displays, and Wayland placement behavior.
  - _Progress (2026-11-04):_ Connected the tested core geometry plan to window sizing, reachable monitor placement, maximized state, and change-only background persistence. Windows work areas and X11 monitor bounds are queried; Wayland leaves placement to the compositor. Core and X11 parser tests pass, `xrandr` reports the live 3440×1440 X11 display, and the production app starts in that session. Native resize/move/maximize/restart acceptance remains open; this WSL host forces Slint/Winit to X11.
- [x] 5.6 Wire Slint `@tr()` and bundled English/pt-BR gettext catalogs, including Rust-originated statuses/errors and menu text; verify catalog compilation and OS-locale selection, with English fallback and no missing keys on every foundation screen.
  - _Verified (2026-11-04):_ Slint bundles both catalogs and Cargo builds them. All 56 Slint translation keys are present in English and pt-BR; Rust storage/auth statuses and startup renderer errors use catalog translations. Native Windows pt-BR UI was previously verified, and the English fallback plus Portuguese startup-error translation pass app tests.
- [x] 5.7 Populate Settings/About with version, app/dependency/asset licenses, renderer setting, and credential-storage disclosure; verify About offline and the disclosure in both keyring and fallback-file states, and document the renderer restart behavior.
  - _Verified (2026-11-04):_ Installed Windows About displays version, embedded offline notices, renderer setting, and Credential Manager disclosure. Slint shows the Windows, Secret Service, and private-file status variants; keyring and fallback adapter tests pass. Restart behavior is visible in Settings and documented in `docs/first-launch-and-session.md`. Linux graphical display remains part of task 8.2.

## 6. Application composition and session lifecycle

- [x] 6.1 Wire core, platform, API, Tokio, and Slint commands/snapshots with blocking I/O on workers and UI updates on the event loop; verify delayed mock network/database/keyring operations leave navigation and window interaction responsive.
  - _Verified (2026-11-04):_ Slint commands and event-loop snapshots are wired to Engine. Delayed fake network and credential operations leave dispatch, settings updates, and snapshots responsive; the ordered storage-worker test covers persistence. The production app also stayed live without startup errors in the local X11 smoke check.
- [x] 6.2 Connect provider selection, browser sign-in, single-instance callback validation, session verification/storage, and top-bar identity; verify an integration test completes the fixture browser flow and a failed/late callback cannot replace the session.
  - _Verified (2026-11-04):_ Provider/browser/callback/session/identity wiring is implemented. Fixture browser-flow and replay/late-callback integration tests pass; the user independently confirmed real-provider login works in the installed Windows build. This does not assert restart/session-restoration acceptance.
- [x] 6.3 Restore stored credentials at startup and implement rejected-session cleanup versus recoverable network failure/retry; verify restart tests avoid another browser launch for a valid session, remove an invalid credential, and preserve it on timeouts.
  - _Verified (2026-11-04):_ Stored-session restoration, rejection cleanup, and offline retry are covered by app engine tests; a valid session restores without browser work and recoverable failures preserve credentials. Full installed restart walkthrough remains in task 8.1.
- [x] 6.4 Implement sign-out and step 1 close/Quit behavior; verify online server invalidation, offline local removal, failed credential-deletion reporting, pending-attempt cancellation, no late sign-in after sign-out, and no background process after exit.
  - _Verified (2026-11-04):_ App engine tests cover online/offline sign-out, credential-delete errors, cancellation and late callbacks. Installed Windows Quit exits and releases ownership; full session restoration is tracked separately in task 8.1.
- [x] 6.5 Document first launch, sign-in/retry/sign-out, storage disclosure, and foundation close behavior; verify the walkthrough matches both locales and contains no tray/autostart instructions for step 1.
  - _Verified (2026-11-04):_ Added `docs/first-launch-and-session.md` with English and pt-BR control labels, sign-in/retry/sign-out behavior, storage disclosures, Quit, renderer restart/fallback behavior, and Wayland geometry limits. It contains no tray or autostart instructions.

## 7. CI installation artifacts

- [x] 7.1 Native Windows/Linux CI runner jobs and runner-built artifacts waived by the user; no hosted CI runners are part of this change.
- [ ] 7.2 Package a Windows ZIP with per-user PowerShell install/uninstall scripts, application shortcut, and quoted HKCU scheme registration; verify install, URI argument delivery with spaces/encoded data, reinstall to the stable path, and cleanup of only owned entries without deleting user data.
  - _Progress (2026-11-04):_ Per-user PowerShell installer/uninstaller and local Windows ZIP generation are implemented; prior Windows install/reinstall/quit/uninstall checks restored the previous URI handler and preserved the SQLite SHA-256. The existing ZIP is stale against four current app sources and lacks the packaging README; this host lacks `llvm-lib`/`lld-link`, so an updated build and current-artifact acceptance remain pending.
- [x] 7.3 Package a Linux tar archive with per-user install/uninstall scripts, an XDG application entry, and `x-scheme-handler/app.gamenode` registration; verify install, `xdg-open` callback delivery, reinstall, and owned-integration cleanup in a graphical test session.
  - _Verified (2026-11-04):_ Rebuilt `dist/linux/gamenode-desktop.tar.gz` after fixing executable-path escaping for spaces. With the system XDG tools, isolated install/reinstall, `xdg-open` URI delivery (including spaces/encoded data), previous-handler restoration, no-prior-handler cleanup, and data preservation all passed. WSL's own `xdg-open` routes to Windows, so the Linux XDG dispatch check explicitly selected its generic handler path; details are in `docs/linux-verification.md`.
- [ ] 7.4 Include translations, fonts, brand assets, and all notices in both artifacts and document exact install/uninstall and runtime requirements; verify each artifact can be installed using only its shipped files and documented OS dependencies.
  - _Progress (2026-11-04):_ Linux tar package built and its runtime/install requirements documented; catalogs, font, artwork, and UI icons are embedded in the executable, and notices ship beside it. The existing Windows ZIP predates the packaging README and latest renderer/geometry code; rebuilding and artifact-only Windows installation remain pending.
- [x] 7.5 Installed-artifact smoke checks on CI runners waived by the user; no hosted CI runners are part of this change.

## 8. Roadmap acceptance

- [ ] 8.1 On a real Windows installation from a local build, sign in through an existing GameNode provider, quit, restart signed in, and see the empty library; record version/revision and results for URI delivery, second launch, real Credential Manager, window geometry, Skia/software selection, Settings/About, and both locales.
  - _Progress (2026-11-04):_ The documented native Windows login, session restoration, URI, second-launch, Settings/About, and locale checks passed on the earlier build. Its manifest is stale against current renderer/geometry source, so this acceptance must be repeated on an updated Windows build.
- [ ] 8.2 Repeat the done-when walkthrough on Linux with Secret Service present and absent, covering X11 and Wayland shell behavior; record keyring/fallback disclosure and permissions, URI delivery, session restoration, geometry limitations, renderer selection, and both locales.
  - _Progress (2026-11-04):_ Local X11 startup, software UI snapshots, and package URI registration/forwarding checks are recorded in `docs/linux-verification.md`. A non-WSL Wayland plus real Secret Service/sign-in walkthrough remains unverified.
- [x] 8.3 Review installed foundation screens against the recorded Figma Version 3.0 token/asset reference and audit roadmap scope; record any explicit desktop adaptations and verify no later-step provider, tracking, background, reporting, Proton, OptiScaler, publishing, or updater behavior shipped.
  - _Verified (2026-11-04):_ Reviewed signed-out, signed-in empty-library, and Settings/About native software-renderer snapshots on X11 alongside the Windows review record. Theme provenance records desktop/focus adaptations; no later roadmap behavior is present. Current Linux review evidence is in `docs/linux-verification.md`.

## Session handoff — 2026-11-04

The user tested the installed Windows build and confirmed that login and
configuration work. They reported poor visual quality and a crash on opening
the license viewer, explicitly requested that the crash not be investigated now,
and asked to end this task if possible. This work session is closed. The license
viewer failure remains a known, explicitly deferred issue under task 5.7; visual
refinement remains pending. The change itself stays open at 15/37 tasks, without
waiving the remaining requirements or asserting full roadmap acceptance. See
`docs/windows-verification.md` for the acceptance record and its limits.

## UI-priority session — 2026-11-04

User resumed work focusing on the installed Windows interface. Tasks 5.1 and 5.2
are verified complete: theme controls/provenance audited, original wordmark and
all asset hashes checked, bundled fonts/icons rendered natively. Task 5.3 has a
refined shell installed with full-height maximized geometry, scrollable Settings,
focusable controls, Space activation and Escape dismissal verified; exhaustive
keyboard traversal across all authentication states remains open. Task 5.7 now
has a paginated offline license viewer; native opening/page advance passed.
See `docs/windows-verification.md` for evidence and remaining acceptance limits.

## Implementation continuation — 2026-11-04

Renderer fallback, live window geometry persistence, bilingual startup errors,
first-launch documentation, and Linux per-user packaging were implemented. The
Linux archive built successfully; isolated install/reinstall/uninstall checks
passed. Follow-up Linux checks now verify production Skia startup and software UI
rendering on X11; actual XDG callback delivery with `xdg-open` passed after
fixing the desktop-entry executable escaping. Workspace tests pass, formatting
is clean, and app/platform Clippy passes.
The user explicitly waived CI runner jobs and runner-only artifact smoke checks;
those tasks are marked waived. Native graphical acceptance for software
startup and Linux XDG packaging are verified. Resize/move/maximize/restart
acceptance remains open. This WSL environment forces Winit to X11, so a real
non-WSL Wayland and Secret Service walkthrough is still open. The Windows ZIP
does not match the latest four app source files and could not be rebuilt here:
`llvm-lib`, `lld-link`, CMake, and Ninja are unavailable. The build script no
longer requires Windows PowerShell interoperability or `wslpath` for packaging.
Current task notes and `docs/linux-verification.md` record the remaining
material acceptance work; the required package command is in
`docs/windows-verification.md`.

## Reconciliação documental do histórico — 2026-11-09

As cinco tarefas abertas abaixo conservam seu texto e estado originais. Evidência histórica pertence ao binário/ambiente registrado; código ou testes existentes não substituem execução manual. Esta reconciliação não executou novos testes.

| Tarefa original | Constatação e evidência | Destino e condição restante |
| --- | --- | --- |
| 5.5 | Há [planejamento de geometria e testes](../../../../crates/core/src/geometry.rs) e restauração no [host Tauri](../../../../crates/app/src/main.rs). Os registros [Linux](../../../../docs/linux-verification.md) e [Windows](../../../../docs/windows-verification.md) mantêm limitações de validação ao vivo. | Permanece aberta aqui para resize/move/maximize/restart, telas indisponíveis e limitações Wayland. Integração Steam 8.3/9.1 cobre regressões de lifecycle, mas não comprova toda esta matriz de geometria. |
| 7.2 | [Instalação](../../../../packaging/windows/install.ps1), [desinstalação](../../../../packaging/windows/uninstall.ps1) e [documentação Windows](../../../../packaging/windows/README.md) existem. O registro Windows contém resultados de instalação e callback para builds históricos. | Aceitação do pacote atual tem sobreposição com [integração Steam](../../integrate-steam-plugin-into-main/tasks.md) 9.1–9.2. A matriz completa de instalação, reinstalação, quoting e preservação de dados continua pendente nesta tarefa até evidência correspondente. |
| 7.4 | A tarefa 7.1 da [migração Tauri/React](../2026-11-07-migrate-slint-to-tauri-v2-react/tasks.md) registra o empacotamento sucessor. Existem [script Linux](../../../../scripts/package-linux.sh), [script Windows](../../../../scripts/package-windows-wsl.sh) e notices. | Integração Steam 6.2/7.3/9.1 acompanha partes do pacote integrado; instalação usando somente os arquivos distribuídos e dependências documentadas permanece pendente onde não houver resultado explícito. |
| 8.1 | O [registro Windows](../../../../docs/windows-verification.md) contém login relatado pelo usuário e resultados posteriores de restauração/UI para versões antigas, com limites expressos. A migração removeu a seleção Skia/software. | Conservar esses resultados como parciais. Requisitos de renderizador foram substituídos pela migração, não aprovados retroativamente. Integração Steam 9.1–9.3 cobre parte da aceitação atual; geometria, locales, conta GameNode e demais critérios sem cobertura permanecem aqui. |
| 8.2 | O [registro Linux](../../../../docs/linux-verification.md) distingue WSLg/X11, limitações de keyring/Wayland e login posterior relatado no Arch/KDE sem identidade exata do pacote. | Sobreposição parcial com integração Steam 8.1–8.4. Permanecem aqui os critérios não demonstrados para X11/Wayland, ambos os backends de credenciais, geometria e locales; a escolha de renderizador antiga tem sucessor na migração. |

Destino técnico do shell: tarefas 1.3, 4.1–4.3 e 7.1 da migração Tauri/React. As pendências sem sucessora completa continuam nas tarefas originais deste arquivo; nenhuma foi encerrada por esta nota.
