# First launch and session basics

> **Status: archived legacy guide.** This describes the discontinued Slint foundation
> build, not the stable Tauri application. Its renderer controls and empty-library-only
> behavior are historical. Use the [current OpenSpec baseline](../openspec/README.md)
> and [development guide](development.md) for current work.

GameNode Desktop opens to the signed-out welcome screen. Choose a provider to
continue in your default browser. After the provider approves access, the browser
returns to GameNode Desktop and the account name appears in the top bar. A
successful sign-in opens the empty library; this foundation build does not scan
for or launch games.

| English control | pt-BR control | What it does |
| --- | --- | --- |
| Continue with Google / Discord / Twitter | Continuar com Google / Discord / Twitter | Opens the selected provider in the default browser. |
| Cancel sign-in | Cancelar entrada | Cancels the current browser sign-in attempt. |
| Retry connection | Tentar conectar novamente | Retries session restoration after a recoverable connection error. |
| Menu → Sign out | Menu → Sair da conta | Removes the local session and asks the server to invalidate it. If the server is offline, the local sign-out still completes and the app reports that confirmation was unavailable. |
| Menu → Quit | Menu → Encerrar | Closes the application and releases its single-instance endpoint. |
| Settings / About | Configurações / Sobre | Opens rendering, session-storage, version, and license information. |

The Settings screen identifies where the session is protected: Windows
Credential Manager, Linux Secret Service, or (on Linux when Secret Service is
absent) a private file readable only by the current user. The app does not use
the file fallback when an existing Secret Service is locked or failing. The
session is not stored in the settings database.

The Software rendering setting takes effect after restarting GameNode Desktop.
If Skia cannot initialize on startup, the app makes one software-renderer
attempt, leaves the saved preference unchanged, and labels the active renderer
as an automatic fallback in Settings. If both renderers fail, the startup error
reports that settings and the saved session were preserved.

Window size and maximized state are restored on the next launch. Saved positions
are restored only when the current window system permits positioning and the
window remains reachable on an attached display. Wayland controls placement;
GameNode restores the size and maximized state there and leaves position to the
compositor.
