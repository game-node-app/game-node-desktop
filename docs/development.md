# Native development

See [CONTRIBUTING.md](../CONTRIBUTING.md) for the commit-message convention used
by developers and AI agents.

Build from the repository root. `rust-toolchain.toml` pins Rust 1.99.1 with
rustfmt and Clippy; Slint and slint-build are pinned together to 1.18.1. Install
Rust using rustup, then let rustup install the toolchain when invoking Cargo.
Keep `Cargo.lock` and use `--locked`. The workspace has exactly four packages:
core, platform, api and app. Core has no Slint or OS adapter dependency.

## Linux (Ubuntu/Debian, native x86-64)

Install native prerequisites:

```sh
sudo apt-get update
sudo apt-get install -y build-essential pkg-config cmake ninja-build clang libclang-dev python3 git curl \
  libfontconfig1-dev libfreetype6-dev libx11-dev libx11-xcb-dev libxcb1-dev x11-xserver-utils \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxrandr-dev libxi-dev \
  libxcursor-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libegl1-mesa-dev libgl1-mesa-dev libvulkan-dev
```

The selected Winit backend supports X11 and Wayland. Running the graphical
application requires a logged-in graphical session, its display connection and
working graphics drivers; software rendering is the foundation's alternative.
Secret Service uses the user D-Bus session and an installed service such as
GNOME Keyring or KWallet. No Secret Service activates the documented private-file
fallback; a locked service must not do so. SQLite is bundled and does not require
a separately installed SQLite development package. The HTTP client uses Rustls,
not OpenSSL. Skia builds can download a matching prebuilt archive; source builds
need the C++/Clang/Python toolchain listed above and network access for Skia sources.

## Windows (native x86-64 MSVC)

Install Visual Studio 2022 Build Tools with **Desktop development with C++**,
the x64 MSVC tools and Windows SDK. Install LLVM (including libclang), CMake,
Ninja, Python 3, Git and rustup. Use an x64 Native Tools command prompt or a
PowerShell session with that developer environment initialized. Ensure these
tools are on PATH; if libclang cannot be located, set `LIBCLANG_PATH` to the LLVM
`bin` directory. Use the `x86_64-pc-windows-msvc` Rust host toolchain. Windows
Credential Manager is the credential backend and has no file fallback. Slint
uses Winit with Skia or software rendering; Qt and WebView are not required.

Native Linux and Windows builds require their corresponding prerequisites above.
This change does not configure hosted CI runners; its verification commands are
run locally, and cross-compilation does not replace native OS acceptance.

## Required checks (both platforms)

```sh
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

## Live UI Screen Studio and hot-reload (WSL / Linux)

For day-to-day UI iteration, visual inspection, keyboard/mouse testing, and
AI-agent verification—including Windows-gated DRM-free and local save-protection
screens—prefer the isolated software-rendered screen studio over cross-building
and reinstalling the Windows package on every edit.

[`crates/app/examples/ui-review.rs`](../crates/app/examples/ui-review.rs) runs
`AppWindow` inside Slint's deterministic `MinimalSoftwareWindow` with in-memory
fixture state (no database, credential store, or network access) and encodes
frames directly to PNG or PPM. [`scripts/dev-screen.sh`](../scripts/dev-screen.sh)
wraps it with a background file watcher and CLI driver.

### Starting from scratch

```sh
./scripts/dev-screen.sh start
```

This builds `ui-review` (`--locked`), starts the HTTP studio on
`http://127.0.0.1:4777`, keeps the latest rendered frame at
`/tmp/gamenode-screen.png`, and watches `crates/app/ui/**`,
`crates/app/examples/ui-review.rs`, and `i18n/**`. Whenever a watched file
changes, it automatically rebuilds and restarts the server while restoring the
active preset, locale, and window size from `/tmp/gamenode-ui-state.json`.

### Driving the screen from the CLI or browser

- **Browser / T3 Code Preview**: open `http://127.0.0.1:4777` to see the live
  1:1 canvas, click/drag/scroll/type directly on the Slint window, switch
  presets/locales/window sizes from the top toolbar, and inspect the live state
  JSON in the side panel.
- **CLI subcommands** (each command updates `/tmp/gamenode-screen.png` and
  prints the updated state JSON):
  ```sh
  ./scripts/dev-screen.sh status                         # Print current screen & property state JSON
  ./scripts/dev-screen.sh preset drm-detail en           # Open a preset (with optional locale and WxH)
  ./scripts/dev-screen.sh preset restore pt_BR 1100x720  # Save protection + restore preview in pt_BR
  ./scripts/dev-screen.sh preset editor en               # DRM-free game settings editor modal
  ./scripts/dev-screen.sh locale pt_BR                   # Switch between en and pt_BR
  ./scripts/dev-screen.sh resize 640x420                 # Test compact (640x420) or wide (1440x900) sizes
  ./scripts/dev-screen.sh click 520 340                  # Click logical coordinates (x, y)
  ./scripts/dev-screen.sh key Escape                     # Send key (Escape, Tab, Return, DownArrow, F5, ...)
  ./scripts/dev-screen.sh type "Celeste"                 # Type text into focused input
  ./scripts/dev-screen.sh shot /tmp/review.png           # Save a PNG or PPM screenshot to a custom path
  ./scripts/dev-screen.sh stop                           # Stop the background server and watcher
  ```
- **One-shot snapshot mode** (without starting the server):
  ```sh
  cargo run --locked -p gamenode-app --example ui-review -- /tmp/shot.png library protection pt_BR
  ```
- **Headless integration tests** (can also dump per-step PPM screenshots when
  `GAMENODE_SHOTS=<dir>` is set):
  ```sh
  GAMENODE_SHOTS=/tmp/shots cargo test --locked -p gamenode-app --test library_screen --test library_pt_br
  ```

### Scope and limitations

The live screen studio verifies `.slint` layouts, responsive sizing, focus
scopes, keyboard navigation, localization (`en` and `pt_BR`), and UI callback
wiring, but it **does not** exercise:
- Native Winit/OS window decorations, DPI scale factors from a physical monitor,
  or the Windows system tray (`ui/windows.slint` / `ui/tray.slint`).
- Skia GPU rendering or automatic GPU-to-software startup fallback.
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

For the full local workflow, run `scripts/install-windows-wsl.sh` from WSL. It
builds the locked Windows MSVC executable, installs it for the current Windows
user, and launches the installed app. It requires WSL Windows interop plus
`powershell.exe` and `wslpath` on `PATH`.

The unsigned development package installs below
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
passed and the installed native app opened in pt-BR using Skia. See
`windows-verification.md` for the local installation evidence and limits.
This checkout has no configured Git remote. Clean-checkout native CI builds
and uploaded CI artifacts remain pending; WSL cross-compilation and execution
are recorded separately from native Windows compilation.
