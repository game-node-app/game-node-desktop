# Design

## Context

See `proposal.md` for motivation and scope. This is a greenfield application: the target repository currently contains the roadmap and OpenSpec setup, with no Rust sources, CI, brand assets, or repository license. There are no main capability specs to reconcile. The user selected **GPL-3.0-only** for the app and Slint.

Read-only inspection of the sibling projects established the authentication contract without carrying over their code:

- `game-node-server/src/auth/auth.service.ts` configures Better Auth at `/v1/auth`, the bearer and Capacitor plugins, and Google, Discord, and Twitter sign-in.
- `game-node-server/src/auth/auth.integration.spec.ts` tests native `capacitor-origin: app.gamenode://`, the authorization proxy, callbacks carrying a `cookie` query parameter, acceptance of the signed session-cookie value as a bearer credential, and sign-out invalidation.
- The installed `better-auth-capacitor@0.4.0` server plugin preserves callback query parameters when adding `cookie`. Its proxy transfers OAuth state into the browser cookie context. These details matter for a Rust client that has no Capacitor runtime.
- `game-node-clients/packages/ui/src/util/theme.ts` confirms Roboto and an existing web theme. The [Figma Version 3.0 reference](references/figma-version-3.md) and its [JSON snapshot](references/figma-version-3-tokens.json), extracted directly through Figma MCP, are the token authority for this change.

Figma access is resolved: `3994:677` is confirmed as the Version 3.0 page. The planning reference records exact values, source variable/style/node IDs and modes, aliases, measured geometry, and the exported GameNode wordmark. The page contains multiple token families; the desktop selection below is explicit rather than treating those families as identical.

## Goals / Non-Goals

**Goals:** Establish enforceable crate boundaries, responsive UI/event handling, durable local state, and a complete browser-to-native session path that can be installed and checked on both target operating systems.

**Non-Goals:** Do not prebuild future provider, tracker, runner, queue, or reporting abstractions. Do not share TypeScript application code or reproduce Figma's web navigation/features. Closing the step 1 window exits the process; close-to-background arrives with step 3.

## Decisions

### 1. Four crates with inward dependencies

Use workspace packages named `gamenode-core`, `gamenode-platform`, `gamenode-api`, and `gamenode-app` at the roadmap paths. `app` depends on the other three; `platform` and `api` depend on `core` types/interfaces where needed. `core` does not depend on the adapters or Slint.

- `core`: foundation settings/user models, authentication state transitions, commands and snapshots, and the small persistence/credential/API interfaces actually required by step 1. It owns application state without direct OS, database-file, network, or UI access.
- `platform`: native credential access, application-data paths, SQLite repository/migrations, single-instance IPC, browser opening, and OS installation/URI integration.
- `api`: the Rust HTTP client and authentication DTOs/errors. No game/library endpoints yet.
- `app`: the binary, Slint views/theme/assets, Tokio runtime, and composition of adapters with core commands/snapshots.

Slint runs on the main thread. Tokio workers perform HTTP, IPC, and blocking-adapter work; rusqlite and keyring calls run on a blocking worker. Snapshots return through Slint's event-loop dispatch using a weak UI handle; controls only emit commands. An all-in-one binary would be initially smaller but would bypass the roadmap's explicit separation.

Pin compatible Rust and Slint versions during workspace setup and commit `Cargo.lock`. Use explicit Windows/Linux feature selection rather than silently enabling unsupported platforms.

### 2. Local data is ready before the window

After acquiring instance ownership, resolve per-user data locations, open rusqlite, apply migrations, and read renderer/geometry settings before selecting the UI backend. The platform repository implements the persistence interface used by core. Begin with schema history and foundation settings only; add game-related tables in their owning roadmap steps.

Each migration runs in a transaction and advances its version only on success. A newer unsupported schema or failed migration stops normal startup and leaves existing data intact. Tests use temporary databases, not the user's data directory. Credentials and callback URLs never enter SQLite. A database reset-on-error is simpler but would discard user state.

### 3. Native shell with a local Version 3.0 theme

Use the Winit backend with Skia for normal rendering and Slint's software renderer for the saved software setting. Select the backend before creating UI components. If GPU setup fails, attempt software rendering once; if backend initialization cannot be retried in-process, use a bounded re-execution that releases and reacquires instance ownership. Never loop or reset the user's settings. The fallback status and selected preference are visible in Settings. Slint documents renderer selection through [BackendSelector](https://docs.slint.dev/latest/docs/rust/slint/struct.BackendSelector) and the [Winit backend](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/).

The UI contains a top bar with the current user, app menu, sign-in state or empty library, and Settings/About. It has no step 2 library toolbar, populated grid/list, provider filters, detail pane, or fabricated game content. Use ordinary desktop focus traversal and shortcuts for existing actions, with visible focus. Persist normal geometry and maximized state; clamp restored geometry to available screens. On Wayland, preserve supported size/state and let the compositor place the window because absolute positioning is not generally available.

At implementation, local assets belong in `crates/app/ui/` and its theme/fonts/icons subdirectories. No remote fonts, generated logo substitutes, web rendering, or runtime Figma dependency.

The imported planning reference records:

| Family | Required source information | Slint use |
| --- | --- | --- |
| Colors | Version 3.0 style/variable names or measured fills/strokes, literal values, opacity, node IDs | Window/surface layers, text, borders, brand accents, control states |
| Typography | Roboto family, used weights, font sizes, line heights, letter spacing | Foundation text roles |
| Spacing | Used design scale or repeated measured gaps/padding, units, source nodes | Desktop shell/control spacing |
| Radii | Used corner values and source styles/nodes | Controls and panels |
| Assets | GameNode logo/icon source nodes or canonical asset provenance and licenses | Bundled brand assets and application icon |

Use `GameNode / Semantic` (Dark) and `GameNode / Dimensions` (Default) as the shared base: accent `#F15025`, background `#161616`, surface `#262525`, default text `#D9D9D9`, dimmed text `#808080`; spacing `10/12/16/20/32`; radii `2/4/8/16/32`; font sizes `12/14/16/18/20`. Their aliases resolve through `GameNode / Primitives`, and their use on the Version 3.0 page was confirmed. Settings status/border tokens and measured chrome/on-primary values fill the remaining roles. The reference preserves the separate Landing V3 values as alternate evidence rather than silently merging their scales or base colors.

Use Roboto body 14/400, supporting text 12/400, labels 14/600, section headings 16/500, page headings 20/500, and the source 13/18 caption style. Keep auto line heights intrinsic. The primary-button reference supplies 14/700 text, 4 px rounding, 24 px inline padding and a 36 px height; center the text vertically because the source's declared 15 px vertical padding conflicts with that fixed height. The Top Bar supplies 54 px height and 16 px horizontal padding. These are visual guidance and reusable values, not requirements to copy the web screen layout.

Copy the [exact wordmark SVG](references/assets/gamenode-logo.svg) into application assets during apply. Preserve its 72.8:28 aspect ratio. Retain source names and IDs in the theme mapping, label measured values separately, and document native geometry/focus adaptations. A checked-in snapshot keeps builds independent of Figma credentials, temporary asset URLs, and future file edits.

### 4. Reuse the mobile wire contract in Rust

The API adapter uses only existing auth endpoints; provider selection uses the server's existing Google/Discord/Twitter choices, not store connections. The sequence is:

1. Core creates a cryptographically random, single-use correlation nonce and a short-lived pending attempt (ten minutes, in memory). Starting another attempt replaces the earlier one. Build an exact callback such as `app.gamenode://home?desktop_attempt=<nonce>` and a corresponding failure callback.
2. POST `/v1/auth/sign-in/social` with provider, callback URLs, and `capacitor-origin: app.gamenode://`. Do not substitute the unrelated MCP OAuth client flow. Preserve the per-attempt state cookie from the response.
3. Open the system browser to `/v1/auth/capacitor-authorization-proxy`, URL-encoding the returned authorization URL and supplying `oauthState` if the state cookie requires it. The proxy establishes the browser's state cookie before going to the provider. The Rust request itself cannot set cookies in the user's browser.
4. Receive `app.gamenode://home?...&cookie=...` through the owner instance. Parse it as data. Check exact scheme/host/path, one correlation value, active attempt, expiry, and unconsumed state. Parse only the expected Better Auth session cookie, including its secure-prefix form; decode once, preserve the signed credential value, and never log the URL.
5. Validate the credential with `/v1/auth/get-session` using `Authorization: Bearer ...`, persist it through the credential adapter, consume the attempt, and publish the signed-in user snapshot. Do not treat a merely present `cookie` field as authentication.

Use a mock HTTP server and redacted fixtures to lock down request headers, proxy state transfer, preserved nonce, cookie parsing, and failure behavior. A real-provider round trip remains a manual acceptance check on each OS. The server deployment must trust `app.gamenode://`; verify that existing deployment configuration when running acceptance, without introducing new server features.

On startup, load the credential and validate it asynchronously. A 401/rejected session clears it; a timeout or network failure preserves it and offers retry. Sign-out invalidates the attempt and removes local identity/credentials even if the server is unavailable, then reports any unsuccessful invalidation/deletion. Keep the current credential only long enough to attempt `/v1/auth/sign-out`; do not create an offline token queue. An embedded browser or new desktop auth service would add scope and ignore the existing contract.

### 5. Credential storage follows the OS

Use keyring with explicitly enabled Windows Credential Manager and Linux Secret Service support under a stable app service/account key. Linux without Secret Service uses an atomically written token file under a private user directory (directory `0700`, file `0600`); avoid symlink traversal. A locked or failing existing keyring is an error, not evidence that Secret Service is absent. Windows has no file fallback. Settings receives a storage-method enum, never the credential or file contents.

This keeps the roadmap's supported Linux fallback while avoiding silently downgrading storage on transient credential-service failures. Cover keyring-success, unavailable-service, locked-store, failed-write, and deletion failure using adapter fakes; check real services in OS acceptance.

### 6. Instance ownership precedes state initialization

Use per-user IPC/ownership: a named pipe with current-user access control on Windows and a private Unix socket under the user runtime directory on Linux. Send only bounded typed messages (`activate` or `auth-callback`). The OS scheme handler passes the URI as one quoted argument; neither launch registration nor IPC invokes a shell with URL content. Validate callbacks again in the owner, since same-user IPC delivery is not proof of an initiated sign-in.

Secondary launches wait briefly for an owner that is still starting, forward their message, and exit. Handle stale endpoints after crashes without allowing simultaneous owners. Callback cold starts have no pending in-memory attempt and therefore cannot sign in; they show the sign-in shell. Persisting unfinished OAuth attempts is unnecessary for step 1's restart requirement, which concerns completed sessions.

### 7. Minimal per-user installation before release packaging

CI builds natively on Windows and Linux. For step 1, package a Windows ZIP plus PowerShell install/uninstall scripts and a Linux tar archive plus install/uninstall scripts. Install to a stable user-owned binary path. Windows registers the application shortcut and `HKCU\Software\Classes\app.gamenode` with a quoted binary/URI argument. Linux installs an XDG desktop entry with `MimeType=x-scheme-handler/app.gamenode;` and registers it through `xdg-mime`.

Uninstall removes only this installation's binaries and owned integration entries, leaving user data intact. Reinstallation updates the handler to the stable installed path. These artifacts exercise installation and auth callbacks now; public installers, signing, AppImage, Flatpak permissions, publishing, and updates remain step 9 work. Administrator/system-wide packaging is unnecessary for the foundation.

### 8. Bundle translations and licenses

Use Slint `@tr()` and gettext `.po` catalogs bundled at compile time, following [Slint's translation guide](https://docs.slint.dev/latest/docs/slint/guide/development/translations/). Use source English and a maintained English catalog plus Brazilian Portuguese; normalize the OS locale and fall back to English. Route Rust-side errors to translated shell messages and translate native menu strings from the same catalogs. No language picker is required in this step.

Add the repository GPL-3.0-only license during implementation, set workspace package license metadata, select Slint's GPLv3 path, and include the dependency and bundled font/asset notices in About and the install artifacts. The user made the application-license choice; Slint's available options are documented in its [license](https://github.com/slint-ui/slint/blob/master/LICENSE.md?plain=1). Do not infer the licenses of the GameNode artwork or Roboto files from the repository license.

## Risks / Trade-offs

- [Version 3.0 includes different token families and some inconsistent control geometry] → Use the recorded semantic/dimension selection, retain alternate raw evidence, and apply the documented native control adaptation without changing roadmap scope.
- [Mobile plugin details change] → Test the observed wire contract with fixtures and verify an actual browser round trip against the selected deployment on both OSes.
- [GPU fallback and Wayland window behavior vary] → Exercise Skia/software, Windows, Linux X11, and Linux Wayland; treat compositor placement as an OS constraint and record it in acceptance.
- [Linux Secret Service is absent or locked] → Distinguish absence from failure, disclose the private-file fallback, and test both service-present and service-absent setups.
- [Hosted CI cannot prove real OAuth or every GPU/credential environment] → Use automated mock/IPC/install smoke checks and record the real Windows/Linux done-when walkthrough separately.
- [Unsigned archives and scripts need explicit installation] → Document exact install/uninstall steps; do not present these as the final public release packages.

## Migration Plan

There is no prior desktop data or code to migrate and nothing is imported from the Electron prototype. Fresh installs apply the initial schema. Later versions use ordered migrations; the foundation tests include a supported older fixture and an unsupported newer-schema fixture. Rolling back binaries is supported only while the database schema is compatible; never delete or downgrade data to force startup.

Implementation acceptance is the roadmap's walkthrough on both OSes: install a CI artifact, sign in through the browser, quit, restart signed in, and see the empty library. Also verify second-launch handoff, callback rejection, renderer persistence, usable geometry, Settings/About, and both locales. No actual build, OAuth session, OS installation, or GUI test is claimed by this planning change.
