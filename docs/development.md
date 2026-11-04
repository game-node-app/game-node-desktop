# Native development

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
`dist/linux/gamenode-desktop.tar.gz`. Extract it and run `./install.sh` to
register the XDG application and URI handler. Run the installed
`~/.local/opt/gamenode-desktop/uninstall.sh` to remove the owned integration
without touching application data.

Use temporary directories for tests; never use production credentials or local
application data. Unit/mock tests do not prove real-provider OAuth, native
installation, keyboard navigation, or graphical X11/Wayland behavior. Record
those separately using the change's acceptance tasks.

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
