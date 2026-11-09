# Building desktop packages with provider plugins

Provider plugins run as separate executables beside GameNode Desktop. Release packages contain the
host and each platform's official provider binaries together; the host never searches `PATH` or a
development target directory for a plugin. Package scripts build from the locked workspace and
check package layout and architecture before creating an archive.

## Linux

From the repository root, run:

```sh
scripts/package-linux.sh
```

The script builds the UI, `gamenode-app`, `gamenode-plugin-steam` and
`gamenode-plugin-packagekit` with `--locked`, then writes a versioned archive under `dist/linux/`.
The package stores all three executables together in `libexec/`. The Linux package manager plugin
uses pacman on Arch-family systems and PackageKit on other distributions. Its discovery is
read-only and receives `provider.context.read` and `library.discovery`; it has native access to
desktop-entry files, local process execution for pacman, and system D-Bus for PackageKit. The
process plugin is not sandboxed. The unversioned `gamenode-desktop.tar.gz` remains a convenience
alias for the latest local package.

## Windows

From WSL with the documented `cargo xwin` toolchain and Windows SDK inputs, run:

```sh
scripts/package-windows-wsl.sh
```

The build step cross-compiles the locked workspace for `x86_64-pc-windows-msvc`. The package keeps
`gamenode.exe` and `gamenode-plugin-steam.exe` together at the package root, with the existing
license notices, installer scripts and required backup assets. It writes the versioned ZIP and
`dist/windows/gamenode-desktop.zip` alias. Install the package from Windows PowerShell with the
included `install.ps1`; run GameNode from its installed location so adjacent executable lookup
continues to apply.

To check an already staged package without executing its binaries:

```sh
python3 scripts/verify-plugin-package.py --platform linux <package-directory>
python3 scripts/verify-plugin-package.py --platform windows <package-directory>
```

The verifier checks the required host and platform plugin binaries, their target format and
architecture, the platform's existing package resources and license notices. It rejects obsolete
daemon/CLI prototypes and rejects the Linux package manager executable in Windows packages.
Windows cross-compilation and archive validation do not replace installing and running the package
in a Windows VM.
