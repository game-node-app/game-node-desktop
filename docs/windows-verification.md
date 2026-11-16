# Local Windows foundation verification — 2026-11-04

Version: 0.1.0 development build, from the current working tree based on
`542d8cd`. This includes uncommitted sources. The local ZIP's
`build-manifest.json` records source hashes and the binary SHA-256.

Build host: Ubuntu 26.04 on WSL2, x86-64. Cargo-xwin 0.23.1, LLVM/lld 21.1.8,
CMake 4.2.3, Ninja 1.13.2 and Microsoft CRT/SDK fetched by cargo-xwin.
Target: `x86_64-pc-windows-msvc`, Rust 1.99.1, Slint 1.18.1.

Verified:

- Locked Windows workspace cross-build succeeded. The 94 Windows tests executed
  natively via WSL interoperability, including process tests for simultaneous
  launches, intact callback forwarding, startup wait and crash recovery.
- Windows tests exposed an unbounded synchronous named-pipe read. Forwarding
  now uses overlapped I/O with a cancellation deadline; the stalled-owner test
  passes. Linux's process test also exposed a directory-creation race; it now
  revalidates a concurrently created directory. All 104 Linux tests pass.
- Installer created the per-user binary and Start Menu shortcut, and registered
  `"<installed binary>" "%1"` under the current user's URI class.
- The installed native window opened as GameNode. UI Automation reported the
  Portuguese welcome/provider controls and keyboard-focusable buttons.
- Settings/About showed version 0.1.0, Skia, Windows Credential Manager,
  GPL-3.0-only, the font/asset notices and the renderer restart control in pt-BR.
  Fonts, images, catalogs and license notices are embedded in the binary.
- An ordinary second launch exited with code 0. A synthetic unsolicited URI
  callback did not authenticate the owner. Exactly one installed process
  remained running.
- The app menu's Quit action terminated the process. Uninstall restored the
  prior Electron URI handler, and the SQLite file SHA-256 was unchanged.
  Installation and reinstallation then succeeded at the same stable path.
  The final app was launched again for the user.

This is a local development installation, not a CI acceptance artifact.
Real-provider sign-in/session restoration, real credential save/delete,
software/GPU-failure recovery, full geometry behavior, complete locale/error
coverage, Linux graphical installation and the final visual audit remain
tracked in OpenSpec. No claim is made that the whole foundation change is done.

## User acceptance and end of this work session

On 2026-11-04, the user independently tested the installed Windows build and
reported that login and configuration work. The user also reported poor visual
quality and that opening the license viewer crashed the application. These are
user observations; the crash was not reproduced or diagnosed by the agent.
The user explicitly requested no investigation of that crash now and asked to
end this task if possible.

This session is closed at the user's request. The successful real-provider login
supports completion of task 6.2 alongside the existing fixture/replay tests.
The report does not establish that restart restoration, sign-out edge cases,
renderer fallback, geometry, both locales, or Linux graphical acceptance passed.

Known follow-up: the license viewer crashes when opened. Investigation and repair
are deferred by explicit user direction; task 5.7 remains open. Visual refinement
and reference verification also remain pending. No application source or installed
binary was changed in response to this report.

The OpenSpec change remains in progress at 15/37 tasks. It is not complete or
archived; the other implementation and acceptance requirements remain tracked.

## UI refinement — 2026-11-04

The user resumed implementation with an explicit UI/installed-Windows priority.
The earlier session closure remains historical. Replaced generic widgets with
local themed controls, a persistent navigation rail, centered authentication and
empty-library states, settings panels, local SVG navigation icons, and translated
supporting copy. The first iteration incorrectly constrained the shell height;
the user's maximized screenshot exposed it. The corrected shell anchors chrome,
navigation and footer to the actual client area and scrolls content independently.

Verified on the actual installed Windows executable with Skia: persisted session
restoration, maximized full-height shell, identity and empty library, Settings,
Credential Manager disclosure, offline license viewer opening and advancing to
page 2, and accessibility keyboard-focusable foundation controls. Space opens the
menu and Escape dismisses it. The notices are now presented in bounded 2400-character
pages, preserving the complete text rather than laying out the whole document.
This verifies the replacement viewer's exercised path, not a diagnosis of the
previous crash. Normal, signed-out and minimum-size settings screens were also
reviewed with an isolated native software-rendered UI harness (`ui-review` example),
which does not access credentials, storage or the network.

Validation: Windows MSVC cross-build, formatting and workspace/all-target Clippy
passed. The app's 17 engine tests passed. Exact wordmark bytes and all seven asset
inventory hashes match; Roboto's variable font embeds the required weights.
Theme provenance records desktop geometry, focus/hover and authored-icon adaptations.
The final binary was reinstalled to the existing per-user path and reopened.

OpenSpec tasks 5.1 and 5.2 are complete (17/37 overall). Full keyboard traversal
across every auth state, Linux acceptance, renderer fallback, geometry persistence,
CI packaging and the other outstanding requirements remain tracked. This session
does not mark the full foundation change complete.

## Packaging and renderer continuation — 2026-11-04

CI runner work was explicitly waived by the user. Linux local X11 startup and
XDG packaging checks are recorded in `docs/linux-verification.md`.

The Windows ZIP currently in `dist/windows/` is stale relative to this working
tree: its build manifest hashes differ for `crates/app/Cargo.toml`,
`crates/app/examples/ui-review.rs`, `crates/app/src/main.rs`, and
`crates/app/ui/app.slint`. The ZIP also predates the packaging README. A fresh
Windows cross-build/package could not be produced in the current host because
`llvm-lib`, `lld-link`, CMake, and Ninja are unavailable. `clang-cl` is present.
The build script no longer requires `powershell.exe` or `wslpath` merely to
package the ZIP. Therefore the earlier installed-Windows results remain valid
for the recorded binary, but they do not verify the latest renderer fallback,
geometry, and Settings/About source changes. The missing build tools can be
installed with `sudo apt install llvm lld cmake ninja-build`.

## Step 3 foundation checks — 2026-11-04

Steam counter parsing and the global Store/Internal source choice use synthetic
fixtures and mock APIs. Native desktop acceptance is recorded separately below.

Verified for the current working tree:

- `cargo xwin check --workspace --all-targets --target x86_64-pc-windows-msvc
  --locked` passed with the Windows-only Slint tray feature enabled.
- The Windows process adapter test executed as a native Windows binary through
  WSL interoperability. It copied a child executable into a temporary folder,
  found its full executable path and creation time, registered a thread-pool
  exit wait, and received one callback with an exit time after creation.
- Linux `cargo tree -p gamenode-app --locked` includes no tray backend. Lockfile
  entries for other targets do not enable that backend on Linux.

Still to execute on a real Windows PC once integration is complete:

- [ ] Validate Steam counter parsing, units, multiple account folders and delayed writes with synthetic fixtures.
- [ ] Install, reboot and verify the tray appears without opening a window.
- [ ] Start an external child process under a scanned Steam fixture install folder and verify Running in the library and tray.
- [ ] Exit the game and verify an accepted fixture counter total at the mock GameNode endpoint,
  and, with Internal selected, a closed measured session in the local queue.
- [ ] Close the window during play: tracking continues; Quit closes the session
  and exits the process.
- [ ] Interrupt power during play, restart, and verify the old session closes at
  its last checkpoint.
- [ ] Turn sync off, play, confirm no playtime is sent; turn it back on and verify
  the stored changed total arrives after the explanation has been dismissed.
- [ ] A background-first start sends nothing before the sync explanation is
  dismissed; test keyboard focus and Escape on the explanation.
- [ ] Disable GameNode in Windows Startup apps, restart GameNode and reboot:
  the disabled choice survives. Uninstall removes the GameNode Run value.

### Automated validation after Steam/source integration

Passed in this working tree on 2026-11-04:

- `cargo test --workspace --locked` on Linux, including the synthetic counter
  fixtures, serialized submission gates, changing a counter while an older
  submission completes, source selection, session close/flush, keyboard controls
  and the sync explanation's focus/Escape behavior.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- `CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env cargo xwin test
  --workspace --target x86_64-pc-windows-msvc --locked`, executing native Windows
  test binaries through WSL interoperability. This includes the process-exit
  test, SQLite source/sync persistence, autostart against an isolated test key,
  and the headless UI tests for Portuguese tray text and keyboard navigation.
- `cargo xwin clippy --workspace --all-targets --target
  x86_64-pc-windows-msvc --locked -- -D warnings`.
- `cargo fmt --all -- --check`, strict OpenSpec validation and `git diff --check`.

The physical desktop, reboot/Startup apps and install/uninstall checklist above
is still pending. Automated tests do not claim those manual results.

### Visible playtime — 2026-11-04

Grid cards, list rows and game details now display the selected source's total;
details also identify the source. Internal history includes the current open
session. An unavailable store total is shown as an em dash rather than zero.

Verified with account-scoped synthetic histories and the UI bindings: 754 Steam
minutes display as 12 h 34 min, source changes preserve separate totals, active
internal time advances, and unknown/zero values remain distinct. The Linux
workspace suite and Clippy passed. Native Windows core/app suites and Windows
workspace Clippy passed, including Portuguese formatting and headless rendering.

Rebuilt the Windows package, invoked the installed app's Quit action through
UI Automation, installed the new package and reopened it. The installed binary's
SHA-256 matched the package binary. The native GameNode window was found, and
UI Automation found seven playtime text elements. No forced process termination
was used. The remaining reboot/Startup apps/manual tray checklist is unchanged.

### DRM-free tracking and handoff checklist \u2014 2026-11-05

Verified in tests and pending physical Windows verification:

- [ ] Add an ordinary DRM-free game executable via **Add game**; launch externally and verify detection within 5-second polling interval.
- [ ] Add a launcher-based DRM-free game; start the launcher and verify:
  - Launcher alone does not initiate gameplay tracking.
  - Spawned child game executable inside root is detected, learned, and tracked.
  - Exiting child executable stops gameplay tracking after 10-second handoff settling, even if launcher stays open.
- [ ] Start learned executable directly from a shortcut without launcher; verify immediate tracking.
- [ ] Test edit-tracking recovery: if an unmonitored executable runs, open **Game settings**, add it to gameplay targets, and verify tracking starts immediately without restarting GameNode or losing historical save archives.
- [ ] Confirm elevated processes (run as Administrator) or processes outside the game root log known inspection limitations rather than guessing identity.

### Desktop library redesign — 2026-11-15

Installed the current release package using `scripts/install-windows-wsl.sh`.
The old installed application was first exited through its Quit action; no
forced process termination was used. The installed executable and packaged
executable had identical SHA-256:
`dcca3fb6b318defd166bbf35cdd3ef650b55dc62053d97be88e1e6c992b2e42a`.
The custom window controls, redesigned sidebar and real cached library were
present in the installed app, independently of the Vite mock.

Native interaction checks used UI Automation, Win32 window measurements and
actual keyboard/mouse input on the Windows desktop at 150% scaling (144 DPI).

- [x] Drag a non-interactive area of the top bar. The window moved from
  `(150,120)` to `(230,180)` without changing its 1900×1150 outer size.
- [x] Double-click the top bar to maximize; restore and maximize controls work.
  The maximized client covered the 3440×1368 working area.
- [x] Minimize sets the native iconic state. Reopening the installed executable
  restores the existing instance rather than creating a second instance.
- [x] Close hides the main window and leaves one application process running.
  Reopening shows the same instance again.
- [x] Resize all four edges and all four corners with mouse input. Each changed
  the expected position/dimensions. Dragging the bottom-right corner below the
  minimum stopped at a 1440×900 physical outer window, equivalent to 960×600
  logical window pixels at 150%. The Win32 client rectangle at that size was
  1418×887 physical pixels; these measurements include the native resize frame
  in the outer window and distinguish it from the browser viewport.
- [x] Quit normally and restart. The old process exited; the new process
  restored the exact outer rectangle `(150,120,1900,1150)` and client size
  1878×1137 at 144 DPI.
- [x] Inspect the top bar at 150%. Its 48 logical pixels render as 72 physical
  pixels; the wordmark, search, sync indicator, account menu and all three
  window controls were visible in the native capture.
- [ ] Inspect the top bar at 100% DPI on Windows.
- [ ] Inspect the top bar at 125% DPI on Windows.
- [x] Record the Snap Layouts tradeoff: the web maximize button does not supply
  the native title-bar hover flyout. Custom chrome retains the app's explicit
  maximize/restore actions; this check does not claim native hover Snap Layouts.

The attempt to open Windows display settings did not expose a usable Settings
window through UI Automation, so no system scaling change was made. The 100%
and 125% checks remain pending. Browser viewport resizing and unit tests are
not substitutes for those checks; OpenSpec task 21.3 remains unchecked.

The native capture is retained locally as
`%TEMP%\gamenode-redesign-native.png`; it contains the real user's library and
is not added to the repository. Browser reference comparisons and integrated
test results are in
`openspec/changes/redesign-desktop-library/acceptance-notes.md`.

### Dialog redesign native acceptance — 2026-11-16 (partial)

Installed the current working tree with `scripts/install-windows-wsl.sh` on
the existing Windows session at 150% scaling. The working tree was based on
`d5292f119cce89985cb755e6fe4851dff4d1dd0f` with uncommitted source changes. The
packaged and installed `gamenode.exe` had the same SHA-256:
`5b46984e35fd2c3d57d95dea09bc82496eb5cbcf919041c634edb293ae8c18b2`.
The installed app opened the real cached library in pt-BR.

Verified on the installed executable:

- [x] Open a DRM-free game's **Game settings**, choose **Change cover…**, and
  select a 900×1200 PNG through the native Windows file picker. The picker
  opened with the PNG/JPEG/WebP filter and returned the chosen file.
- [x] The host wrote a lossless WebP cover named
  `local-d1b57c6a-19f4-42b9-a4a2-11ced0add9be-e9b31920e4a3d9ed.webp` in the
  cover cache; its VP8L header reports 600×800. The editor displayed it and
  **Save** persisted `customCover` in the registration.
- [x] The custom cover took precedence in the library tile and the detail pane.
- [x] Reopening the editor and choosing **Use the GameNode cover**, then
  **Save**, cleared `customCover` and deleted the local WebP; the GameNode
  cover returned in the tile and detail pane.

Not executed in this native session, so task 11.5 remains open:

- [ ] Reopen the browser during a pending sign-in. The installed app has an
  active real GameNode session; testing this path would require signing out,
  and the session could not be restored without user credentials. No credential
  handling, sign-out or session change was performed.
- [ ] Open in Steam from the achievement list.
- [ ] Backup retention with the bundled Ludusavi helper.

This record proves the native image picker, re-encode, save and clear paths. It
does not prove browser reopen, the provider achievements page or Ludusavi
retention on Windows.

