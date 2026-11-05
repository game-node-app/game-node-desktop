# Local Linux foundation verification — 2026-11-04

Build host: Ubuntu 26.04 under WSLg, x86-64. Rust 1.99.1, Slint 1.18.1.
The user installed `libxkbcommon-x11-0`, `xdg-utils`, and
`x11-xserver-utils` before these checks. X11 reports one connected 3440×1440
display.

Verified:

- The production Linux binary started with its default Skia renderer, created
  its isolated application database, and remained alive without startup errors
  until the five-second smoke-check timeout.
- The Winit software-rendered UI review harness launched and rendered the
  signed-out, empty-library, and Settings/About screens on X11. The snapshots
  were visually reviewed. Settings displayed the active software renderer.
- The Linux release tar archive rebuilt successfully with its binary, icon,
  notices, README, and install/uninstall scripts.
- With a temporary home directory, the shipped installer registered through
  `xdg-mime`; reinstall stayed at the same path with spaces. The generic XDG
  `xdg-open` path launched the registered executable and passed an
  `app.gamenode://` URI containing encoded spaces and shell syntax unchanged.
  Uninstall restored an earlier handler, removed a registration when there
  was no earlier handler, and preserved user data.
- Core geometry and Linux X11 display-parser tests pass; `xrandr` reports the
  active display. The app's production startup also succeeded on this X11
  session.

Limits:

- WSL's `xdg-open` detects WSL and routes schemes to Windows through
  `rundll32.exe`. To exercise Linux desktop-entry dispatch, the isolated test
  selected `DE=generic`. The dispatch test substituted a capture wrapper at
  the installed executable path; Linux IPC tests separately cover intact URI
  forwarding.
- Slint's Winit integration explicitly selects X11 under WSL because the WSL
  Wayland compositor may restart. Unsetting `DISPLAY` therefore did not provide
  a Wayland session. A non-WSL Wayland walkthrough remains unverified.
- This host did not have a real Secret Service session. Linux credential
  fallback, permissions, and keyring behavior are covered by adapter tests, but
  actual sign-in/session restoration and both live storage states remain open.
- Window geometry planning and persistence are implemented and unit-tested;
  live move/resize/maximize/restart interaction remains open.

## Step 3: playtime and background acceptance

Needs a native Linux machine; the native desktop acceptance run is left to the
Linux milestone. WSL cannot establish these native Linux acceptance results.
No item below is marked as executed by this change's automated checks.

- [ ] Build and launch as in step 2; scan, matching, covers, Play and Add remain
  usable, with the existing renderer and credential behavior.
- [ ] Closing the window quits and releases the single-instance endpoint.
- [ ] Tracking reports unavailable. No measured sessions are recorded, no running
  indicator appears in grid/list/detail, and Play has no Running restriction.
- [ ] No tray backend or tray icon is enabled for Linux.
- [ ] No autostart entry is registered or changed (including XDG autostart and
  systemd user units).
- [ ] Sync preferences persist across restarts and remain keyboard accessible.

The process adapter's Linux unit test asserts `Unsupported` for listing and
watching. This tests the adapter contract; it does not replace the native
window-close and integration checks above.

## Issue #2: experimental tar launcher

Implementation checks used throwaway directories under `/tmp/opencode`, isolated
HOME/XDG paths and a capture executable instead of the graphical application.
No host desktop association was changed and no container or VM infrastructure
was introduced.

Verified:

- Bash syntax and whitespace checks.
- Release build and tar packaging via `scripts/package-linux.sh`; archive
  inspection confirmed the current scripts, launcher execute permissions,
  binary, artwork and license notices.
- Launcher argument forwarding, repeat launch without rewriting the desktop
  entry, conflicting installations, relocation, registration failure, portable
  cleanup, stable-path installation and cleanup with unrelated files retained.
- Real generic `xdg-mime` registration/query/cleanup and `desktop-file-validate`.
- Real `gio launch` desktop-entry dispatch with spaces, quotes and a dollar sign
  in the executable path, preserving the complete synthetic callback URI.

GIO could not resolve an executable path containing a percent sign despite a
valid desktop entry. The launcher and installer therefore reject such paths
with an explicit message rather than claim successful integration.

These are script/integration smoke checks, not Arch/KDE or real-provider login
acceptance. The full package walkthrough in
[the implementation plan](linux-tar-launcher-plan.md) remains pending; the user
startup result below covers only part of it.
Unlike the earlier installer, this launcher refuses foreign protocol associations
by default. Explicit `--force` consent permits replacement, with restoration on
uninstall only if the previous desktop entry still exists and the user has not
chosen another default since.

Follow-up smoke checks covered force-option consumption, help and `--`, actionable
conflict guidance, remembered handlers, restoration, later user choices and
continued file-ownership protection. Real `xdg-mime` and Qt 6 were also exercised
with an isolated KDE environment and a substituted KDE cache command, including
GIO callback dispatch through a package path with spaces. This is not a live
KDE-session acceptance run. The registration subprocess locates Qt's unversioned
`qtpaths` under `/usr/lib/qt6/bin` when absent from PATH. The desktop entry uses
`/usr/bin/env` as its fixed executable so older xdg-mime parsers can resolve
launchers whose paths contain spaces.

Error-dialog selection was checked with substituted `kdialog` and `zenity`
commands: KDE-tool preference, Zenity fallback, missing tools, and headless
execution all preserve stderr and a failing exit status. No real host dialogs
were opened; visibility after a file-manager launch remains a manual check.

### User-reported graphical startup

The user reported successfully opening the release package's launcher by
double-clicking the script in the file manager. The application restored the
existing session after a few seconds; the supplied screenshot shows the signed-in
library. Reference environment: Arch Linux, KDE Plasma 6.8.2, Wayland, Firefox
159.0 and kernel `7.2.14-zen1-1-zen`. The exact package commit was not recorded.

This confirms launcher startup and existing-session restoration, not a fresh
Firefox sign-in, browser protocol prompt, or graphical error dialog. Those
checks remain pending. The user had also performed manual cleanup of an earlier
development association, so this is not proof of an untouched first installation.

After this test, the user agreed to move the packaged binary into `libexec/` to
make the launcher the obvious entry point. See the
[packaging decision](linux-tar-launcher-plan.md#decision-keep-the-packaged-binary-under-libexec).
The layout change was subsequently implemented and checked as described below;
the user-reported graphical result above still refers to the old layout.

### Distributed build identity

The versioned release package was built successfully. Inspection confirmed that
the build version agrees across the embedded binary string, `VERSION`,
`build-manifest.json` and archive filename. All packaged-file hashes, the archive
SHA-256 sidecar and the latest-build alias were verified. An isolated installation
with a substitute executable also confirmed metadata copying and removal.
Visual display of the longer version in the real UI remains a manual check.

### libexec package layout

The rebuilt release archive places the binary only at `libexec/gamenode`; the
root launcher and desktop entry remain the user-facing entry points. Isolated
checks with a capture executable covered exact callback forwarding, real GIO
dispatch, paths with spaces, relocation, portable cleanup, stable-path install
and upgrade from the managed flat layout. Cleanup preserved unrelated files in
`libexec`, and installation/uninstallation refused symlinked binary directories.
A missing internal binary produced an error instead of falling back to a stale
root binary. Archive file hashes, permissions and checksum were verified.

Double-click startup, session restoration and a fresh browser login with this
new layout still need graphical acceptance. Extract portable upgrades into a
fresh directory, since extracting tar archives does not remove obsolete files.
