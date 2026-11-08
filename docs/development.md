# Native development

See [CONTRIBUTING.md](../CONTRIBUTING.md) for the commit-message convention used
by developers and AI agents.

Build from the repository root. `rust-toolchain.toml` pins Rust 1.99.1 with
rustfmt and Clippy. Install Rust using rustup, then let rustup install the toolchain
when invoking Cargo. Keep `Cargo.lock` and use `--locked`. The workspace has
exactly four packages: core, platform, api and app. Core has no Tauri or OS adapter dependency.
Node.js (v24+) and pnpm are used for the `ui/` frontend workspace.

## Linux (Ubuntu/Debian, native x86-64)

Install native prerequisites:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config cmake ninja-build clang libclang-dev python3 git curl \
  libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf
```

The desktop application is powered by Tauri v2 with WebKitGTK on Linux. Running
the graphical application requires a logged-in graphical session and display connection.
Secret Service uses the user D-Bus session and an installed service such as
GNOME Keyring or KWallet. No Secret Service activates the documented private-file
fallback; a locked service must not do so. SQLite is bundled and does not require
a separately installed SQLite development package. The HTTP client uses Rustls,
not OpenSSL.

## Windows (native x86-64 MSVC)

Install Visual Studio 2022 Build Tools with **Desktop development with C++**,
the x64 MSVC tools and Windows SDK. Install LLVM (including libclang), CMake,
Ninja, Python 3, Git and rustup. Use an x64 Native Tools command prompt or a
PowerShell session with that developer environment initialized. Ensure these
tools are on PATH; if libclang cannot be located, set `LIBCLANG_PATH` to the LLVM
`bin` directory. Use the `x86_64-pc-windows-msvc` Rust host toolchain. Windows
Credential Manager is the credential backend and has no file fallback. Tauri v2
uses WebView2 on Windows.

Native Linux and Windows builds require their corresponding prerequisites above.
This change does not configure hosted CI runners; its verification commands are
run locally, and cross-compilation does not replace native OS acceptance.

## Required checks (both platforms)

```sh
pnpm --dir ui build
cargo metadata --no-deps --format-version 1
cargo check --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo build --workspace --locked
```

To create the unsigned Linux per-user tar package from a native Linux build, run
`scripts/package-linux.sh`; it writes
`dist/linux/gamenode-desktop-<build-version>-<target>.tar.gz`, with a SHA-256
sidecar and a latest-build alias at `dist/linux/gamenode-desktop.tar.gz`.
The package build version is shared by the UI, `VERSION` and
`build-manifest.json`; it includes the workspace version, Git revision, dirty
state and UTC build time. Python 3.11+ is needed by the packaging script.
Extract it and run `./gamenode.sh` to
register the per-user XDG application and URI handler before launching. Use
`./install.sh` instead for a stable-path installation. The matching
`uninstall.sh` removes owned integration without touching application data.
See `packaging/linux/README.md` for experimental Arch + KDE support boundaries
and `docs/linux-tar-launcher-plan.md` for issue #2 acceptance checks.

Use temporary directories for tests; never use production credentials or local
application data. Unit/mock tests do not prove real-provider OAuth, native
installation, keyboard navigation, or graphical X11/Wayland behavior. Record
those separately using the change's acceptance tasks.

## Frontend Development and Preview Workflow (Vite & T3 Code Preview)

For day-to-day UI iteration, visual inspection, keyboard/mouse testing, and
AI-agent verification—including DRM-free and local save-protection screens—prefer
the Vite development server and interactive browser preview over cross-building
and reinstalling the Windows package on every edit.

The frontend is located in `ui/` and is built with React 19, `@tauri-apps/api`,
`@tanstack/react-query`, `@tanstack/react-virtual`, `@tanstack/react-store`,
Tailwind CSS 3.4, Mantine v9, and `@tabler/icons-react`.

### Architecture Guardrails

- **Async Tauri Commands**: Handlers implemented in Rust must be asynchronous
  `#[tauri::command]` functions that offload heavy computation or blocking I/O to
  worker threads, keeping IPC responsive.
- **Custom Asset Protocol**: Cover images and artwork are loaded via the custom
  `gamenode-asset://` protocol (`gamenode-asset://localhost/cover/<file>` and
  `gamenode-asset://localhost/artwork/<game_id>`). In React, render covers using
  `<img loading="lazy" decoding="async" />`.
- **Virtualized Views**: The game library grid and list views utilize `@tanstack/react-virtual`
  for high-performance DOM virtualization with large game collections.
- **State & Query Bridge**: Use `@tanstack/react-query` to bridge engine snapshot events
  and query cache updates, and `@tanstack/react-store` for client-side UI state.

### Styling with Tailwind CSS and Mantine

Tailwind handles layout and visual styling; Mantine supplies accessible overlay and
form primitives. Both use the Version 3.0 design tokens shared with
`game-node-clients`.

- `ui/tailwind.config.ts` defines the utility tokens: `brand`, `paper`, `secondary`,
  `tertiary`, `chrome`, `chrome-border`, `surface`, `raised`, `border-subtle`,
  `text-default`, `text-dimmed`, `danger`, `success` and `warning`, plus the
  project spacing, radii, font sizes and control dimensions.
- `ui/src/styles/theme.css` keeps the `@font-face`, the Version 3.0 CSS variables and
  the CSS layer order: Mantine styles first, Tailwind base inside the `tailwind`
  layer, and Tailwind utilities last so utilities win when applied.
- `ui/src/AppProviders.tsx` mounts `MantineProvider` with the dark `appTheme`, the
  brand palette and the GameNode variant color resolver. Render overlays inside that
  provider; standalone component tests need their own `MantineProvider` wrapper.
- Prefer utility classes for layout, colors, typography, borders, radii and
  `hover`/`active`/`focus-visible` states. Keep inline `style` only for runtime
  values such as virtualizer transforms and sizes, measured sidebar/detail-pane
  widths, pointer-anchored menu coordinates and computed status colors.
- Use `Menu`, `Notification`, `Switch` and `SegmentedControl` from Mantine for
  complex controls.
- Use the shared `FloatingModal` (`ui/src/components/FloatingModal`) for large work
  surfaces that benefit from dragging and resizing. It provides header dragging,
  bottom-right resizing, viewport clamping, keyboard resize, geometry persistence
  and focus trapping.
- Use the shared `AppModal` (`ui/src/components/AppModal`) for centered confirmation
  and notice dialogs.
- Both modal primitives use one layer stack: the last modal opened renders on top,
  only the topmost surface handles Escape, and closing restores focus to its trigger
  (`useFocusRestore`).
- Use `@tabler/icons-react` components instead of loose SVG assets.
- Keep `@tanstack/react-virtual` as the library virtualization engine. Do not wrap
  virtual tiles or rows in Mantine layout components; style their containers with
  Tailwind classes instead.

### Running the Vite Development Server

```sh
pnpm --dir ui dev
```

This starts the Vite development server on `http://localhost:4777` with instant HMR.

### Mock authenticated library for UI work

Run the frontend with the isolated mock host enabled:

```sh
pnpm --dir ui install --frozen-lockfile   # first time, or after dependency changes
VITE_UI_MOCK=1 pnpm --dir ui dev
```

Restart an already running Vite server with the variable set; the mode is selected
at startup. Open `http://localhost:4777/`. It starts with a fictional signed-in account and
12 fictional games, local cover images, and a verified library. Search, the filter panel
(availability, running, match state, play history, last played, release decade, date
added and favorites), active-filter chips, sorting by title, store, match status, release
year, playtime, last played or date added, grid/list view, selection, details, favorites
(the star in the grid, list, detail pane and context menu), basic matching, the add
dialog and sign-out/sign-in update the in-memory snapshot. Reload to reset it. Use
`http://localhost:4777/?scenario=signed-out` to start signed out, add `&library=large`
for 328 owned Steam games (18 installed, a few family-shared), and add
`&steam=expired` (or any store scenario) plus `&locale=pt-BR` (or `?locale=pt-BR` on
its own) for Portuguese.

The mock replaces the Tauri command and snapshot-event bridge, including when
Vite is displayed inside a native dev window. Unsupported commands produce a
console warning and do nothing; they never fall through to the real host.
This is a UI fixture, not an HTTP server or a real authenticated API: it does
not exercise OAuth, remote endpoint contracts, native adapters, or persistence.
The native Rust host still starts normally if you run it, so use the browser for
an isolated UI session. The switch only works in Vite development mode; normal
production builds and ordinary `pnpm --dir ui dev` keep their existing behavior.

#### PC ports mock

`VITE_UI_MOCK=1` also presents a fictional Windows signed-in build with a PC ports
catalog, so the `EXPLORE` destination is visible in a browser running on Linux or
macOS. `?platform=linux` hides it, matching a real Linux build, and
`?scenario=signed-out` hides it because the destination requires a session.

The catalog fixture has seven fictional entries: a guided recipe with a required
game folder and a console-package option, a plain release, a pre-release-only
entry, a withdrawn entry, an installed entry with an update available, an
installer-only entry with no Windows asset, and one entry whose install always
fails so the failure details can be checked. A failed install from a previous
session starts unseen, which gives the sidebar badge a count of one. The catalog
starts from a cached snapshot marked stale, so opening PC ports shows the
refreshing state and then the ready state. `?ports=unavailable` starts with no
snapshot and the unavailable state.

Installing an entry is simulated in memory with `ports:progress` events
(download, verify, extract, supplied files, finalize, register, complete). Cancel
a running install to see the cancelling and cancelled states. "Hollow Mountain:
Deep Delve" always fails with extractor output; the other installable entries
complete and gain an installed provenance row. Picking a parent folder, picking
supplied files, choosing an executable, marking results seen and remembering the
grid/list view and filters all round-trip through the mock host. Reload to reset.

### Running the native app

The Tauri host embeds the built UI (`ui/dist`) through the default `custom-protocol` cargo
feature, so build the frontend first and run the host:

```sh
pnpm --dir ui build
cargo run -p gamenode-app
```

Without that feature Tauri treats the build as a dev build and loads `devUrl`
(`http://localhost:4777`), which shows a blank "Operation was cancelled" window when no Vite
server is running. For native HMR, start `pnpm --dir ui dev` and run
`cargo run -p gamenode-app --no-default-features`.

### Application logs

The desktop host writes a rotating log file under the data directory, alongside the database:

| OS | Log directory |
| --- | --- |
| Linux | `~/.local/share/gamenode-desktop/logs/` |
| Windows | `%LOCALAPPDATA%\GameNode\Desktop\logs\` |

One file per UTC day (`gamenode.YYYY-MM-DD.log`); the newest ten are kept. The default filter logs
`info` for GameNode crates and `warn` for third-party crates, and writes happen on a dedicated
worker thread. Set `GAMENODE_LOG` (or `RUST_LOG`) to change it for a run, for example:

```sh
GAMENODE_LOG="debug,gamenode_api=trace" cargo run -p gamenode-app
```

Settings → About shows the current path and offers **Show recent log** (with a copy button) and
**Open log folder**. Steam diagnostics log the request endpoint, HTTP status, and a failure
category such as `timeout` or `connect`; the cookie, the Web API token, and full URLs are never
written.

### Driving and previewing from T3 Code Preview

- **Browser / T3 Code Preview**: open `http://localhost:4777` via `preview_open`
  (`target: { kind: "environment-port", port: 4777 }`) to preview and interact with
  the UI directly.
- **Automated Frontend Tests**:
  ```sh
  pnpm --dir ui test --run
  ```
- **Building Frontend Assets**:
  ```sh
  pnpm --dir ui build
  ```

### Scope and limitations

Vite browser preview verifies React components, styling, responsive sizing, focus
scopes, keyboard navigation, and localization (`en` and `pt-BR`), but it **does not** exercise:
- Native OS window decorations, DPI scale factors from a physical monitor, or the native
  Windows system tray.
- Real OS adapters (`gamenode-platform`): Windows Credential Manager, Secret
  Service, native file dialogs, Windows process/parent tracking, directory-change
  notifications, registry access, or Ludusavi helper execution.

Use the WSL-to-Windows test and packaging routes below to verify those native
platform integrations.

## Scanner fixtures and running against a local Steam

Scanner tests run over an in-memory file system (`MemoryFiles`, `FixedRoots`), so they behave the
same on every OS. The files in `crates/core/src/provider/steam/fixtures/` follow the layout Steam
writes; ids, paths and sizes are made up:

```text
<steam root>/steamapps/libraryfolders.vdf        fixtures/libraryfolders_{linux,windows,old}.vdf
<steam root>/steamapps/appmanifest_<id>.acf      fixtures/appmanifest_portal2.acf, _malformed.acf
<steam root>/steamapps/common/<installdir>/      an empty folder (a manifest without it is skipped)
```

To add a case, put the new file in `fixtures/`, include it with `include_str!` in
`steam/tests.rs`, and build the file system with the `game(...)` helper there.

To run the app against a throwaway Steam, build the layout under a temporary home folder and start
the app with that `HOME` (Linux; the Steam roots are `~/.steam/steam` and
`~/.local/share/Steam`). The scratch home also keeps your own data, settings and credentials out of
the run: you sign in again inside it, and `XDG_DATA_HOME` and `XDG_CONFIG_HOME` must not point
elsewhere.

```sh
fixture=$(mktemp -d)
steam="$fixture/.steam/steam"
mkdir -p "$steam/steamapps/common/Portal 2"
sed "s#/home/me/.local/share/Steam#$steam#; s#/mnt/games/SteamLibrary#$fixture/Second#" \
  crates/core/src/provider/steam/fixtures/libraryfolders_linux.vdf > "$steam/steamapps/libraryfolders.vdf"
cp crates/core/src/provider/steam/fixtures/appmanifest_portal2.acf "$steam/steamapps/appmanifest_620.acf"
HOME="$fixture" XDG_DATA_HOME= XDG_CONFIG_HOME= cargo run --locked -p gamenode-app
```

The library lists Portal 2. Play and Install hand `steam://` links to the operating system; without
Steam installed the app shows that the Steam client could not be found. Rescan after editing the
fixture (add or delete a manifest) to see games appear and leave. The second library folder does not
exist, which shows the skipped-unplugged-drive case. Delete `$fixture` when done.

On Windows the root comes from `HKCU\Software\Valve\Steam` (`SteamPath`); the registry test in
`gamenode-platform` writes and removes a value under a test key, and a manual run can point
`SteamPath` at a folder laid out as above.

## Build and run Windows executables from WSL

WSL interoperability can launch the actual Windows binaries; a native Windows
Rust/Visual Studio installation is not required for this development route.
Install `cargo-xwin` (`cargo install cargo-xwin --locked`), add the Rust
`x86_64-pc-windows-msvc` target, and provide LLVM/Clang, lld-link, llvm-lib,
CMake, Ninja and NASM on PATH. `cargo-xwin` downloads the Microsoft CRT/SDK.
On Ubuntu these tools are available from clang/llvm/lld, cmake, ninja-build and
nasm packages. Tools may also be extracted into a user cache instead of being
installed system-wide; CMake's shared libraries must then be discoverable.

```sh
scripts/package-windows-wsl.sh
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER=/usr/bin/env \
  cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked
powershell.exe -NoProfile -ExecutionPolicy Bypass -File \
  "$(wslpath -w "$PWD/dist/windows/gamenode-desktop/install.ps1")"
```

### Windows desktop with frontend hot reload

To run the current workspace in a Windows Tauri window without replacing the
installed application, quit GameNode completely (including its system-tray
process), then run from WSL:

```sh
scripts/dev-windows-wsl.sh
```

The launcher builds the Windows development executable under
`target/windows-hmr`, prepares its backup assets, starts a local Vite server,
and opens the Windows app. It forces the real Tauri bridge rather than the UI
mock. It uses the same Windows Credential Manager entry and local application
data as the installed app, so an existing session can be restored and changes
made during testing affect that data and account. If sign-in is needed, the
Windows `app.gamenode://` URI handler must be registered; the normal Windows
installation registers it and forwards the callback to the running dev app.

Keep the launcher running while editing React, CSS, or other Vite frontend
files; Vite updates the window. Rust and bundled provider changes require
quitting the app and running the launcher again to rebuild it. Quit via the
app menu or system tray to stop the development session and Vite; closing the
window can leave the app in the tray. The launcher refuses to start if another
GameNode process or a server on port 4777 is running. If interrupted with
Ctrl+C, quit any development app still running in the tray before restarting.
The normal installed executable and URI registration are not replaced.

The launcher needs the WSL build tools listed above, Node.js, pnpm, Python 3,
`curl`, `flock`, and Windows interoperability (`powershell.exe` and
`wslpath`). The first build can take several minutes and preparing backup
assets can download the project's pinned inputs. Windows must be able to reach
WSL's `http://localhost:4777`; if the check fails, inspect WSL localhost
forwarding and Windows firewall settings.

For the full local workflow, run `scripts/install-windows-wsl.sh` from WSL. It
builds the locked release Windows MSVC executable, installs it for the current
Windows user, and launches the installed app. It requires WSL Windows interop plus
`powershell.exe` and `wslpath` on `PATH`.

The unsigned package installs below
`%LOCALAPPDATA%\Programs\GameNode Desktop`, creates a Start Menu shortcut and
registers a quoted URI command under HKCU. An existing URI handler is exported
as `previous-uri-handler.reg` and restored by uninstall when this installation
still owns the scheme. Reinstall to the same path after quitting the application.
To uninstall, invoke the installed `uninstall.ps1` with PowerShell's
`-NoProfile -ExecutionPolicy Bypass -File` arguments. The execution-policy
setting applies to that PowerShell process; it does not change machine policy.
Application data is preserved.

## Current verification

On 2026-11-04, 104 Linux tests and 94 actual Windows tests passed. The Linux
locked check, formatting and Clippy passed; the Windows MSVC-target cross-build
passed and the installed native app opened in pt-BR. See
`windows-verification.md` for the local installation evidence and limits.
This checkout has no configured Git remote. Clean-checkout native CI builds
and uploaded CI artifacts remain pending; WSL cross-compilation and execution
are recorded separately from native Windows compilation.
