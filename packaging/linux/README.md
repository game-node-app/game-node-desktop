# GameNode Desktop for Linux

Run `./install.sh` from this directory to install for the current user. The
installer places the application under `~/.local/opt/gamenode-desktop`, adds an
XDG application entry, and registers `app.gamenode://` for this user. Run the
installed `uninstall.sh` to remove those owned files and restore the previous
URI handler when one was registered. Application data under
`$XDG_DATA_HOME/gamenode-desktop` is preserved.

The binary embeds the English and Brazilian Portuguese catalogs, Roboto font,
GameNode artwork, and app UI icons. The shipped `LICENSE` and `licenses/`
directory contain the corresponding license notices.

Run in a supported X11 or Wayland desktop session with working graphics drivers
and the system libraries for Fontconfig, FreeType, XKB common, X11 or Wayland,
EGL and Vulkan. X11 sessions also need `libxkbcommon-x11`. Install `xdg-utils`
for URI registration. On X11, `xrandr` is
optional and is used to validate and restore positions across connected monitors;
without it, window placement is left to the window manager. Linux Secret
Service is optional; when no service exists, the app discloses its private-file
credential fallback in Settings.
