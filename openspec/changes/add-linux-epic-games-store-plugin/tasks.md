# Tasks

This change depends on `add-epic-games-store-plugin`. Archive that change first; this one extends
the same `desktop-epic-games`, `desktop-plugins` and `desktop-providers` capabilities. Windows
behavior and Windows acceptance are not reopened.

## 1. Native launcher discovery spike

- [ ] 1.1 With the native Epic Games Launcher AppImage installed (observed preview `0.120.0`),
  locate where it records installed games, install locations, completion state, DLC and engine
  builds. Record exact paths, formats, version behavior and what is unavailable when the launcher
  has no games. Do not read cookies, tokens, IndexedDB/localStorage contents or other private
  WebView state.
- [ ] 1.2 Decide the Linux discovery source and failure model: missing launcher is a successful
  empty scan; unknown/changed layout is an explicit provider error that preserves previous records;
  no arbitrary folder guessing. Document the adapter bounds (entries, records, file sizes).

## 2. Platform and packaging

- [ ] 2.1 Add `linux` to `providers/epic.toml` platforms and Linux roots while leaving the Windows
  registry/ProgramData roots unchanged. Add the Linux descriptor/platform support and keep the
  same provider/plugin identity.
- [ ] 2.2 Build and stage `gamenode-plugin-epic` on Linux development/release layouts; update
  install/uninstall and package verification. Keep the Windows package layout and requirements
  unchanged.

## 3. Discovery adapter

- [ ] 3.1 Implement the bounded Linux discovery adapter from the spike, including DLC, incomplete
  install and engine-build filters, stable entry keys (`epic:<appName>`) and install locations.
- [ ] 3.2 Verify a missing launcher is empty-success, a malformed/unknown preview layout fails the
  provider without clearing previous records, and a valid installed game appears with the expected
  record fields.

## 4. Account, catalog and playtime

- [ ] 4.1 Reuse the Linux WebView authorization-code login, per-provider secret slot, refresh
  rotation and provider-neutral account card. Verify sign-in, cancel/close, restart restore,
  sign-out and missing-credential failures without reading launcher cookies.
- [ ] 4.2 Verify owned-library sync, namespace matching, Epic playtime totals (`epicgames`) and
  unknown last-played behavior on Linux. Verify no Windows regressions.

## 5. Launcher actions

- [ ] 5.1 Detect whether `com.epicgames.launcher://` has a desktop handler. With a handler, Play
  and Install open the registered launcher; the launcher owns Proton.
- [ ] 5.2 Without a handler, return an actionable integration error and do not spawn the AppImage,
  Wine/Proton or an arbitrary executable. Document AppImageLauncher/manual desktop-entry
  integration.

## 6. Tests and acceptance

- [ ] 6.1 Add fixtures for Linux discovery (installed, DLC, incomplete, engine build, missing
  launcher, changed layout), account/catalog/playtime and the handler-present/handler-missing
  actions. Add a packaging test that requires the Epic plugin on Linux.
- [ ] 6.2 Run the focused Rust/UI/packaging checks and `openspec validate --strict`.
- [ ] 6.3 On a packaged Linux host with the AppImage integrated and one user-installed Epic game,
  record sign-in, library/catalog, install/play through the launcher (Proton handled by Epic),
  playtime and the missing-handler failure. Keep Windows acceptance evidence unchanged.

## 7. Follow-up

- [ ] 7.1 After acceptance, update the archived `add-epic-games-store-plugin` 10.2 note to point
  at this change's Linux acceptance instead of "no Epic UI on Linux".
- [ ] 7.2 Proton/Wine configuration, automatic AppImage integration and macOS support remain future
  proposals; they are not tasks here.
