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
