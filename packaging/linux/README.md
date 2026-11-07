# GameNode Desktop for Linux

## Experimental support

The initial acceptance target is Arch Linux x86_64 with KDE Plasma on Wayland.
Other environments may work but are not validated. This archive is not a
distribution-independent runtime: compatible system libraries are required.
Package acceptance is pending; the initial local-binary login was checked with
Plasma 6.8.2, Firefox 159.0 and kernel `7.2.14-zen1-1-zen`.

## Extract and run

Extract the complete archive, then run `./gamenode.sh` from its directory in a
graphical user session. No `sudo` is needed. The launcher creates the menu entry,
registers `app.gamenode://` for the current user, verifies the association, and
starts GameNode. The binary is internal, at `libexec/gamenode`; launching it
directly skips this preparation. Use `gamenode.sh` or the GameNode Desktop menu
entry for normal use.
The browser may ask permission to open GameNode when completing sign-in.

```text
gamenode-desktop/
├── gamenode.sh          # Run this launcher
├── libexec/gamenode     # Desktop host
├── libexec/gamenode-plugin-steam # Official Linux Steam discovery plugin
├── VERSION
└── build-manifest.json
```

The desktop process owns the Engine and profile. It hosts the Linux Plugin API and
starts the official Steam discovery plugin only when a scan requests it. Both
executables are packaged together; no separate daemon or CLI is included.

When upgrading from a package with the binary at its root, extract into a fresh
directory rather than overlaying the old archive: tar extraction does not remove
obsolete files. Remove the old registration using its uninstaller before switching
directories. Stable-path installation upgrades remove the managed old root binary.

Keep the extracted directory while using the application. If you move it, run
`./gamenode.sh` from the new location to update registration. Paths with spaces
are supported; percent signs, backslashes, tabs and line breaks in paths are not.
Percent signs are rejected because desktop launchers can fail to resolve an
executable containing them even when the desktop entry passes validation.

Alternatively, run `./install.sh` to copy the package into the stable location
`~/.local/opt/gamenode-desktop` and register it there. Choose this option before
registering the extracted package. To switch between existing installations,
first run the currently registered installation's `./uninstall.sh`. Conflicting
associations are not silently replaced. Legacy installations without directory
metadata must also be removed using their original uninstaller first.

### Existing or stale protocol association

If the launcher reports another handler (for example,
`gamenode-desktop-dev.desktop` from a development setup), explicitly authorize
replacement with:

```sh
bash ./gamenode.sh --force
```

For stable-path installation, use `bash ./install.sh --force` instead. No manual
editing of `mimeapps.list` or cache deletion is necessary. The flag only allows
replacing the protocol association; it does not bypass file ownership or a
conflict with another live installation using the same desktop entry. Launcher
options are consumed rather than forwarded to GameNode; `--` ends option parsing.
Use `--help` for a summary.

The previous handler is remembered and restored on uninstall if its desktop
entry still exists and GameNode remains the default. A stale development handler
whose file no longer exists is not restored. Subsequent user choices are preserved.

On KDE 6, some `xdg-utils` versions call `qtpaths` without finding Qt's binary
directory. When necessary, the scripts add `/usr/lib/qt6/bin` to PATH only for
the registration command. If `qtpaths` is unavailable there too, an actionable
error is shown instead of hiding the dependency failure.

Run the matching `./uninstall.sh` to remove desktop integration. For an extracted
package, the package files are retained. For a stable-path installation, binaries
are removed while license files, ownership metadata (to permit reinstallation)
and unrelated files are retained. Application
data under `$XDG_DATA_HOME/gamenode-desktop` is preserved. Later changes to the
protocol's default handler are not reset.

If registration fails, check the reported error and rerun the launcher after
fixing it. The launcher does not start the application with unverified integration.
In a graphical session, errors are also shown using `kdialog` (preferred) or
`zenity`, when available, so failures are visible without a terminal. On Arch
with KDE, install `kdialog` for this behavior (`sudo pacman -S kdialog`). Neither
tool is a mandatory runtime dependency: without them, errors remain on stderr
and the launcher must be run from a terminal to read them. Whether double-clicking
a shell script executes it also depends on the file manager's security settings;
the launcher cannot display anything if the file manager never runs it.

## Identify the package

The base version comes from the workspace `Cargo.toml`. Each tar build adds the
Git revision, `.dirty` when tracked or untracked source changes exist, and a UTC
build timestamp, for example:

```text
0.1.0+g123456789abc.dirty.20261104T120000000000Z
```

This exact build version appears in the application and `VERSION`. The versioned
archive also includes the target triple. `build-manifest.json` records the base
version, build version, full revision, dirty state, UTC time, target and SHA-256
hashes of packaged files. The adjacent `.sha256` file verifies the archive.
Include the build version when reporting an issue; a dirty build is not fully
identified by its base commit alone. These timestamps identify builds, not
bit-for-bit reproducible artifacts or proof of authenticity.

The unversioned `gamenode-desktop.tar.gz` is only a latest-build compatibility
alias. Keep the versioned archive for comparing or sharing test builds. Ordinary
Cargo builds without the packaging version override still display the workspace
base version.

The binary embeds the English and Brazilian Portuguese catalogs, Roboto font,
GameNode artwork, and app UI icons. The shipped `LICENSE` and `licenses/`
directory contain the corresponding license notices.

Run in a supported X11 or Wayland desktop session with working graphics drivers
and the system libraries for Fontconfig, FreeType, XKB common, X11 or Wayland,
EGL and Vulkan. X11 sessions also need `libxkbcommon-x11`. Install `xdg-utils`
for URI registration (`sudo pacman -S xdg-utils` on Arch). This is an explicit
dependency even if `gio` is already available. `desktop-file-utils` is optional
for cache updates and desktop-entry validation. On X11, `xrandr` is
optional and is used to validate and restore positions across connected monitors;
without it, window placement is left to the window manager. Linux Secret
Service is optional; when no service exists, the app discloses its private-file
credential fallback in Settings.
