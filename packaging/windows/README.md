# GameNode Desktop for Windows

Requires 64-bit Windows 10 or later. Extract this package, then run
`powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1`. It installs for
the current user under `%LOCALAPPDATA%\Programs\GameNode Desktop`, creates a
Start Menu shortcut, and registers `app.gamenode://` under HKCU. If another app
already owns that URI scheme, the installer preserves its registration and the
uninstaller restores it when GameNode Desktop still owns the scheme.

Run the installed `uninstall.ps1` with the same PowerShell options to remove the app and its owned integration.
The application data directory `%LOCALAPPDATA%\GameNode\Desktop` is preserved.
The executable embeds its English and Brazilian Portuguese catalogs, Roboto
font, GameNode artwork, and UI icons. The shipped `LICENSE` and `licenses/`
directory contain the corresponding license notices. Windows Credential
Manager stores the session; no WebView runtime is required.
