# Building desktop packages with provider plugins

The Steam provider runs as a separate executable beside GameNode Desktop. Release packages must
contain both binaries in that same directory; the host never searches `PATH` or a development
target directory for a plugin. The package scripts build the host and Steam plugin from the locked
workspace and run a layout/architecture check before creating the archive.

## Linux

From the repository root, run:

```sh
scripts/package-linux.sh
```

The script builds the UI, `gamenode-app` and `gamenode-plugin-steam` with `--locked`, then writes a
versioned archive under `dist/linux/`. The package stores `gamenode` and
`gamenode-plugin-steam` together in `libexec/`, alongside the existing launcher, license notices
and install scripts. The unversioned `gamenode-desktop.tar.gz` remains a convenience alias for the
latest local package.

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

The verifier checks the required host and Steam binaries, their target format and architecture,
the platform's existing package resources and license notices. It rejects obsolete daemon/CLI
prototypes and the deferred Linux package-manager plugin. Windows cross-compilation and archive
validation do not replace installing and running the package in a Windows VM.
