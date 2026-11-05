# GameNode Desktop: MVP roadmap

Started 2026-11-04. This repository is the desktop app's home, separate from the TypeScript monorepo
(`game-node-clients`). It replaces the Electron prototype on `feat/desktop-client-mvp` and its roadmap. Nothing is
carried over from that code; the product direction below is restated here so this file stands on its own.

MVP = a native Windows app, written in Rust with a Slint UI, that finds the games installed on this PC across stores,
launches them, tracks playtime in the background, reports to GameNode, and installs OptiScaler per game.

Linux follows as its own milestone after the Windows MVP (see [Linux milestone](#linux-milestone-after-the-mvp)).
Development happens in WSL, which can build and unit-test Linux code but cannot exercise a real Linux desktop: no
Wayland session, no Secret Service, no tray host, no Steam or Proton. Windows builds, by contrast, run and can be
tested on the same PC. Linux work resumes when a real Linux setup is available.

Each step below is meant to be planned and shipped as one change.

## Direction

- **A unified launcher, like Playnite.** One library of every game on this PC, whatever the store. Play, track,
  tweak. Installing, updating and downloading stay with each store's own client: Install opens that client on the game.
- **A separate product for a separate audience.** The web and mobile apps are a Letterboxd-style tracker. Desktop is
  for people who want one launcher for everything, connected to their GameNode account. It does not mirror web
  features, and web features do not have to fit it.
- **Desktop data only.** Desktop does not use the web connections or the importer (`/v1/connections/*`,
  `/v1/importer/*`). Every game in the desktop library comes from a desktop provider. Two parallel sources of "your
  games" in one UI is the confusion we are avoiding.
- **Installed games only, for now.** Providers read what is on disk. Logging into stores to list owned-but-not-installed
  games is a later provider capability (see Later).
- **Tracking runs in the background.** GameNode starts at login, on by default, and records playtime for every
  detected game, whether it was launched from GameNode, the store's client or a desktop shortcut.
- **Desktop reports back to GameNode.** Detected games and playtime are sent to the server, on by default with a
  setting to turn it off. The app works fully with it off. This is how desktop-only stores (Ubisoft, EA, Battle.net)
  reach the user's GameNode profile. Reported ownership is self-declared and must not feed anything trust-sensitive.
- **Windows first, Linux-ready.** `core` stays free of OS calls, and every OS-specific piece sits behind a trait in
  `platform`, so the Linux milestone adds adapters rather than reworking the app.

## Desktop UI principle

GameNode Desktop is a native program with GameNode's look, not the website in a window. Screens are designed for
desktop use: keyboard first, context menus, multi-select, split panes, remembered layout, and a local cache so the
library opens without the network.

Nothing is shared with the TypeScript monorepo as code. What we share with web and mobile is:

- **The server API.** Desktop talks to the same GameNode server through its own Rust client.
- **The design tokens.** Colors, type scale, spacing and radii are copied as values into a Slint theme.
- **The brand assets.** Logos, icons and fonts.

## Stack

- **Rust**, one Cargo workspace, toolchain pinned in `rust-toolchain.toml`.
- **Slint** for the UI, using the Skia renderer. A setting switches to Slint's software renderer for machines where
  GPU rendering fails.
- **Tokio** for networking, scanning and process work. The UI thread only runs Slint.
- **SQLite** (`rusqlite`) for local data: scan results, matches, the playtime queue, settings and layout.
- **The OS keyring** (`keyring`) for the session token: Windows Credential Manager. (Step 1 also built the Linux
  Secret Service adapter and its private-file fallback; they are verified in the Linux milestone.)
- **Translations** through Slint's `@tr()` and gettext `.po` files. English and Brazilian Portuguese at launch.

### Repository layout

```
Cargo.toml              workspace
crates/
  core/                 library model, providers, manifests, matching, tracker, playtime queue, sync rules.
                        No OS calls and no UI: everything platform-specific comes in through traits.
  platform/             OS adapters: process listing and exit waits, registry, keyring, autostart, URL scheme,
                        single instance. Windows for the MVP; the Linux adapters from step 1 stay.
  api/                  GameNode HTTP client and types.
  app/                  the binary: Slint UI (ui/*.slint, theme, fonts, icons), tray, wiring.
providers/              bundled provider manifests (*.toml), embedded in the binary.
i18n/                   .po translations.
docs/
```

### Linux code during the MVP

- **It keeps compiling.** WSL is the development host, so the workspace builds on Linux and `core` tests run there.
- **New platform features are Windows-only.** A trait added after step 1 gets a Windows implementation; on Linux it
  returns an "unsupported" error, or the feature is compiled out. No Linux adapter is written until the Linux
  milestone.
- **Nothing Linux-specific is verified by hand.** Linux checks in planned changes move to the Linux milestone.

### How the app runs

- **One application, one persistent running instance.** Starting GameNode while it already runs hands over to the running instance:
  it opens the window, or delivers a sign-in callback.
- **Two modes in that process.** At login it starts in the background (`--background`): tracking and a tray icon, no
  window. Opening GameNode shows the window. Closing the window returns to the background; it does not quit.
  Quit is in the tray menu and the app menu.
- **The window is a view.** `core` owns all state and publishes snapshots; the UI renders them and sends commands
  back (`scan`, `launch`, `match`, ...). The UI never touches files, processes or the network itself.
- **Tray.** Shown while GameNode runs in the background.
- **Autostart.** A `Run` entry under HKCU.

### Sign-in

Uses the server's existing mobile sign-in flow: the system browser opens the provider's page, and the server
redirects back to `app.gamenode://` with the session. GameNode registers that scheme itself under HKCU
`Software\Classes`. A callback is only accepted if it matches the sign-in GameNode started.

### Security rules

- **No web content.** The app renders no HTML. Its fixed-purpose Ludusavi v0.31.0 helper ships with the Windows package, with pinned release digests and notices; it is never downloaded or updated at runtime.
- **Launch is restricted.** A launch is either a URI whose scheme the provider's manifest declares, or an
  executable inside the game's scanned `installDir`, started with a fixed argument list. No shell strings.
- **Writes into game folders are task-specific.** OptiScaler install/uninstall writes only its recorded files. Explicit save restore can replace previewed file/registry targets only after confirmation, a fresh stopped check and verified recovery of current targets. Backup never edits live saves.
- **Downloads come from fixed GitHub release sources over HTTPS** (OptiScaler; later umu and GE-Proton), and are
  checked against published checksums when the release has them.

## Providers

A provider is everything the app knows about one game source: a store, a launcher like Heroic, or manual adds. It is
described by a manifest; parsing that a manifest cannot describe lives in a native scanner in `core`.

### Manifest

Manifests are TOML: they are written by hand, and will later be written by users (see Later), so comments matter.
Bundled manifests live in `providers/` and are embedded at build time. A manifest lists the platforms it supports;
the format keeps per-platform roots so Linux entries can be added in the Linux milestone without a format change.

```toml
# providers/steam.toml: data in the manifest, parsing in a native scanner
id = "steam"
name = "Steam"
platforms = ["windows"]                            # "linux" added in the Linux milestone
source = "steam"                                   # GameNode playtime source
scan = { native = "steam" }                        # libraryfolders.vdf + appmanifest_*.acf
playtime = { counter = "steam" }                   # Steam's own total, if step 3's spike proves it readable
launch = { uri = "steam://rungameid/{id}" }
install = { uri = "steam://install/{id}" }
match = { external_category = 1 }                  # GET /v1/game/external

[[roots.windows]]
registry = 'HKCU\Software\Valve\Steam'
value = "SteamPath"

# Linux milestone:
# [[roots.linux]]
# path = "~/.steam/steam"
# [[roots.linux]]
# path = "~/.var/app/com.valvesoftware.Steam/.steam/steam"   # Flatpak Steam
```

```toml
# providers/ubisoft.toml: fully declarative
id = "ubisoft"
name = "Ubisoft Connect"
platforms = ["windows"]
source = "ubisoft"                                 # added by the server in step 6
launch = { uri = "uplay://launch/{id}/0" }
match = { by_name = true }                         # until the server has a Ubisoft category (step 6)

[scan.registry_subkeys]
key = 'HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs'
fields = { id = "$key", install_dir = "InstallDir" }
```

Rules:

- **A scanner returns plain records** `{ id, name?, install_dir, ...fields }`. The runtime fills the manifest's
  `{placeholders}` from those fields, so native and generic scanners are interchangeable.
- **Generic scanners are added when a bundled store needs one.** Step 4b needs `registry_subkeys` (Ubisoft, GOG) and
  `files` (a glob plus a JSON reader: Epic `.item` manifests). Others (`sqlite`, `vdf`) only when a store requires
  them.
- **Native scanners are for formats a manifest cannot describe:** Steam's KeyValues files, Battle.net's protobuf
  `product.db`. Each one has fixture tests built from real, anonymized files.
- **Manifests are validated when loaded.** An invalid manifest is skipped and logged; it never stops the others.
- **The library key is `<provider>:<id>`.** The same game installed from two stores is two entries pointing to one
  GameNode game.

### Matching

- **Known category:** if the manifest has `match.external_category`, look the game up with `GET /v1/game/external`
  (category plus store ids, batched). Existing categories: Steam 1, GOG 5, Epic namespace 1001.
- **No category yet:** search GameNode by name, prefilled with the detected title, and let the user pick. The pick is
  stored locally and reported in step 6.
- **No match:** the game stays playable. The detail pane offers a manual match at any time.

## Playtime

How the server stores playtime:

- One `UserPlaytime` row per (user, game, source, platform). The game's total is the sum across rows.
- Connection syncs overwrite their row with the store's total, and write a history entry when the values change.

Desktop sends playtime in one of two ways, set by the provider's manifest:

- **A store counter, sent as a total.** When the provider can read the store's own total (Steam's
  `localconfig.vdf`), desktop submits it after the game exits through the existing `POST /v1/playtime`.
  - It counts every PC, and play from before GameNode was installed.
  - It writes the same row a web Steam connection writes, with the same number, so the two never conflict.
- **Measured sessions, added to the total.** Every other provider. The tracker records each session as
  `{ session_id, game_id, source, platform_id, started_at, ended_at }` in the local queue until the server accepts it.
  - The server adds each session to the same (game, source, platform) row and writes a history entry.
  - `session_id` is generated by the client, so retries are harmless. Sessions from two PCs add up.
  - If the user also has a web connection for that store (Epic, GOG), the next connection sync overwrites the row with
    the store's total, which already includes those sessions. A session can count twice only until that sync.

Rules both ways follow:

- `platform_id` is always 6 (PC), as the server's Steam sync uses, so desktop and connection rows line up.
- The source is the store that sells the game, not the launcher that runs it. An Epic game run through Heroic is
  `epicgames`.
- Desktop never sends sessions for a provider that has a store counter.
- Play from before install is not recovered for measured providers. Store logins (Later) can add counters for them.

### Tracker

- **One generic tracker for all providers.** A game is running while any process runs from inside its `install_dir`.
- **Starts are found by listing processes** periodically, through native APIs (Toolhelp). **Exits are found by
  waiting on the process handle**, so a session ends when the game actually exits.
- **Sessions survive crashes.** An open session is written to SQLite when it starts and checkpointed while it runs;
  after a crash or power loss it closes at its last checkpoint. Sessions longer than 24 hours are split.
- **Sessions belong to the signed-in account.** Signing out closes the open session; another account never sees or
  sends the previous account's queue.
- **Process listing and exit waits are a `platform` trait.** The Linux milestone implements it with `/proc` and
  `pidfd`, including Wine and Proton processes, whose Windows paths are translated through the prefix's drive mapping.

## Assumptions

| Question | Assumed answer | Affects |
| --- | --- | --- |
| macOS in the MVP? | No. | Step 8 |
| Linux in the MVP? | No. Windows only. Linux is its own milestone once a real Linux setup is available. | All steps |
| GameNode login required? | Yes. Matching, playtime and reporting go to the user's account. | Step 1 |
| Background tracking at login | On by default. A setting to turn it off comes later. | Step 3 |
| Slint license | GPLv3 if this repository is open source, otherwise Slint's royalty-free license. Decided in step 1. | Step 1 |
| Windows code signing | Decided in step 8 (Azure Artifact Signing vs OV certificate). | Step 8 |

Change an assumption and the matching step changes; the order does not.

## Order

```
1 Foundation ─► 2 Steam library ─► 3 Playtime + background ─┬─► 4a DRM-free + local backup → 4b Epic, GOG, Ubisoft ─┬─► 5 EA, Battle.net
                                                            │                                └─► 7 OptiScaler
                                                            └─► 6 Reporting to GameNode (server)
                                                                              8 Windows release ◄── all

                                                         Linux milestone ◄── Windows MVP + a real Linux setup
```

The first tester build is the end of step 3. Step 6's server work can start at any time; each store added later
reports through it. Steps 5 and 7 can run in parallel after step 4b.

## Step 1: Foundation

**User gets:** nothing on its own. This is the base every later step builds on.

- Cargo workspace with the four crates, `rust-toolchain.toml`, formatting and lint config.
- Build and test for Windows (cross-built from WSL). Unsigned until step 8.
- Slint app shell: the window with GameNode's theme (tokens, Roboto, logos), app menu, remembered window size and
  position, and the software-rendering setting.
- Single instance and the `app.gamenode://` scheme.
- Sign-in through the browser, session in the keyring, sign-out, and the signed-in user in the top bar.
- SQLite store with migrations, and Settings with an About page (version, licenses).
- Translations wired with English and Brazilian Portuguese.
- Decide the Slint license with the repository's own license.

Step 1 also built Linux adapters (Unix-socket single instance, Secret Service with a private-file fallback, XDG
scheme registration, a tar package). They stay in the tree and keep compiling. Their remaining live checks (Wayland,
a real Secret Service, sign-in and session restore) move to the Linux milestone.

**Done when:** a Windows build installs, signs in, restarts signed in, and shows the empty library.

## Step 2: Steam library

**User gets:** their installed Steam games in a real library, matched to GameNode, launchable.

- Provider runtime in `core`: manifest loading and validation, the scanner registry, placeholder filling, launch rules.
- Steam: manifest plus native scanner for `libraryfolders.vdf` and `appmanifest_*.acf`, with fixture tests. Windows
  roots only; the scanner itself is OS-neutral, so Linux and Flatpak roots are a manifest change later. Play uses
  `steam://rungameid`; Install opens `steam://install`.
- Matching through `GET /v1/game/external` (category 1), with the prefilled search for unmatched games.
- Covers from GameNode, cached on disk.
- Library UI: grid and list, sidebar with provider filters, search, sort, keyboard navigation, multi-select, context
  menus, the detail pane with Play, Install and match, and remembered layout.
- Add to GameNode: the detail pane adds the game to the user's GameNode library through collection entries, the same
  API the web uses. Detected games are never added automatically.
- Rescan on start, when the window opens, and on demand.

**Done when:** on Windows, a tester sees their installed Steam games matched to GameNode, matches one by hand,
launches one, and adds one to their GameNode library.

## Step 3: Playtime and background mode

**User gets:** the first tester build. GameNode runs in the background from login and puts Steam playtime on their
GameNode profile.

- The tracker, as described above, with "running" shown in the library.
- The local session queue. Measured sessions wait in it until step 6 ships the sessions endpoint.
- Steam playtime spike: read Steam's own counter (`userdata/<id>/config/localconfig.vdf`, per Steam account on the
  PC) and record which fields, units and update timing it has. If it works, Steam submits its total through
  `POST /v1/playtime` with source `steam`, platform 6. If it doesn't, Steam uses measured sessions like every other
  provider.
- Background mode: `--background` at login, the tray icon and menu, autostart through HKCU `Run`, and
  close-to-background.
- "Sync with GameNode" setting, on by default and explained on first run. Off means nothing leaves the PC except the
  lookups needed for matching.

**Done when:** after a reboot, GameNode is in the tray without being opened; a Steam game started from Steam itself
is shown as running; and its playtime appears on GameNode (or is queued, if the spike chose measured sessions).

## Step 4a: DRM-free games and local backups

**User gets:** Add game from one Windows executable selection, immediately playable
without choosing catalog metadata. Conservative background identity matching keeps
manual corrections and explicit no-match choices authoritative.

- Stable manual installation UUIDs, editable launch/tracking settings and Locate/Remove.
- Opt-in watched folders produce a review queue; no discovery adds games automatically.
- Explicit gameplay executable ownership, external starts and bounded launcher handoff.
- Bundled pinned Ludusavi helper and baseline manifest, isolated per-install save profiles,
  local after-play backup, ten full changed versions and explicit recoverable restore.
- Local save protection is independent of GameNode sync. No cloud saves, store scanner
  or store authentication is introduced by this step.

**Done when:** ordinary and launcher-based DRM-free games add, match/correct, launch,
track while hidden and back up after physical exit; isolated file/registry restore
and failure recovery pass Windows acceptance.

## Step 4b: Epic, GOG and Ubisoft

**User gets:** the common Windows stores in the same library, including Ubisoft.

- Generic scanners `registry_subkeys` and `files`, each with tests.
- Epic: local launcher manifests, namespace matching (category 1001) and declared URI launch.
- GOG: `GOG.com\\Games` registry data, category 5 matching and Galaxy/direct launch.
- Ubisoft: registry discovery and user-correctable catalog matching.
- Measured sessions reuse step 4a's process policies where appropriate.

**Done when:** an Epic, a GOG and a Ubisoft game each appear, launch and record sessions.
Steps 5 and 7 depend on this store integration portion; step 6 server work remains independent.

## Step 5: EA and Battle.net

**User gets:** the rest of the major Windows launchers.

- EA app and Battle.net: native scanners (study each format first: EA's install data, Battle.net's `product.db`),
  launched through their clients.

**Done when:** an EA and a Battle.net game launch and record sessions.

## Step 6: Reporting to GameNode

**User gets:** desktop-found games and playtime on their GameNode profile, on web and mobile too, including stores the
web cannot connect to.

Server work (`game-node-server`):

- **Playtime sessions endpoint:** `POST /v1/playtime/sessions`, as described in Playtime. Idempotent on
  `session_id`; rejects sessions that end in the future, have `ended_at <= started_at`, or last over 24 hours.
- **New playtime sources:** `ubisoft`, `ea`, `amazon` and `manual`. `battlenet` and `emulator` already exist.
  (`amazon` is used by Heroic in the Linux milestone; adding it now keeps the server change in one place.)
- **New GameNode-owned external categories:** Ubisoft, EA and Battle.net (the next ids after
  `NintendoTitleId = 1002`). Their unmapped ids go through the existing `registerUnmappedGames(category, entries,
  userId)`, and moderators map them with the existing tools.
- **Reported games endpoint:** `PUT /v1/desktop/games` with `{ provider, store_id, name, game_id? }[]`.
  - `game_id` is the match the user picked on desktop. It applies to that user only; global mappings stay with
    moderators.
  - Reports are a sticky union per user: entries are added or updated, never removed by a later report. Uninstalling
    doesn't change ownership, and whether a game is installed is not sent.
  - Manually added games are not reported.
- **A "GameNode Desktop" connection:** a connection type with no account behind it, created by the first report. Web
  and mobile show it in the connections list; its importer lists the reported games with their store. Deleting it
  deletes the reported games; playtime stays.

Desktop work:

- Report the detected list after every scan, and send queued sessions and store totals, while "Sync with GameNode" is on.
- Matching moves from name picks to the new categories as moderators fill them.

Decided:

- **No device identity.** Sessions add up across PCs without one, and reports are a sticky union, so two PCs never
  overwrite each other.
- **Detected games are not added to the GameNode library automatically.** Installed is not the same as played or
  wanted.

**Done when:** a Ubisoft game found on desktop shows up in the web importer under GameNode Desktop; its playtime
appears on the user's profile; and a session recorded offline is counted once after reconnecting.

## Step 7: OptiScaler

**User gets:** one-click OptiScaler per game, reversible, and a clear warning where it could get them banned.

- Anti-cheat block: warn if the game folder contains EasyAntiCheat or BattlEye, or the game is on our blocklist.
  An explicit warning before any install.
- Version picker; download the release `.7z` from GitHub, cache it, and extract it with a Rust 7z library. Show the
  source link and the GPL-3.0 license.
Let the user select stable or nightly.
- Place the files next to the main exe as `dxgi.dll` (fallback `winmm.dll` / `version.dll`); keep the
  `OptiScaler.ini` name.
Check the Optiscaler wiki for the supported filenames, and use the first one.
- Back up overwritten files and record installed files, so uninstall restores the folder exactly.

**Done when:** install then uninstall on a test game leaves the folder byte-identical; an EAC or BattlEye game shows
the warning before install; OptiScaler loads in-game.

## Step 8: Windows release

**User gets:** a trusted public Windows download, and updates.

- Signed Windows installer (Azure Artifact Signing or OV certificate, decided here). Signing also reduces antivirus
  false positives from step 7.
- In-app updates for the installer. Pick the updater here.
- Releases from version tags in this repository: a tag builds, signs and publishes the installer.

**Done when:** a tagged release publishes the signed Windows installer, and it installs, signs in, launches a game,
records its playtime, and updates to the next tagged version.

## Linux milestone (after the MVP)

Starts once the Windows MVP ships and a real Linux machine (and ideally a Steam Deck) is available for testing. It
will be planned as its own set of changes; this is the scope carried over from the original roadmap.

- **Foundation on Linux.** Finish step 1's Linux walkthrough: Secret Service present and absent (private-file
  fallback and its disclosure), X11 and Wayland, sign-in and session restore, URI delivery, renderer selection
  (including the software fallback on NVIDIA setups), both locales.
- **Steam on Linux.** Native and Flatpak Steam roots in the manifest, with fixtures; Proton, runtime and
  redistributable entries filtered out.
- **Tracker on Linux.** Process listing through `/proc`, exits through `pidfd`, and Wine/Proton processes matched
  through the prefix's drive mapping.
- **Background mode on Linux.** XDG autostart `.desktop` file (Background portal under Flatpak). On desktops without
  a tray host (GNOME without the AppIndicator extension), GameNode still runs, and launching it again opens the
  window.
- **Flatpak spike.** From inside a test Flatpak sandbox, launch through `steam://`, see host game processes (native
  and Proton), run host commands, and autostart through the Background portal. Record what host access each needs;
  this decides the Flathub permissions.
- **Heroic and Lutris.** Heroic: `files` over its installed lists for Epic, GOG and Amazon, using the original
  store's category and playtime source, launched through `heroic://launch`. Lutris: native scanner over its database,
  launched through `lutris:rungameid`; source `emulator` when its runner is an emulator, `manual` otherwise.
- **Proton runner.** Download the umu zipapp from its GitHub releases on first use (tell the user if Python 3 is
  missing); download and manage GE-Proton releases in the app's data folder with a default version and a per-game
  picker; one Wine prefix per game, launched with `umu-run`; Settings lists and removes downloaded Proton versions.
  Manual Windows exes and GOG offline installers become playable on Linux.
- **OptiScaler on Linux.** Set `WINEDLLOVERRIDES=<dll>=n,b` for umu launches; for Steam games show the launch-options
  line with a copy button.
- **Linux release.** AppImage with in-app updates, and Flatpak on Flathub with the host access the spike proved
  necessary. Tags publish them alongside the Windows installer. A Steam Deck Game Mode doc: add GameNode as a
  non-Steam game.

**Done when:** on a real Linux desktop and on Steam Deck desktop mode, the Windows MVP's done-when checks pass, a
manual Windows exe launches through umu and GE-Proton and records sessions, OptiScaler loads under umu, and the
AppImage and Flathub builds install, sign in, launch a game and record its playtime.

## Later (designed for, not scheduled)

- **Background tracking setting.** Turn autostart and background tracking off from Settings. Until then, the OS's
  own startup settings can disable it.
- **Store logins: owned-but-not-installed games.** A provider capability to list owned games, with a login flow in
  the system browser where the store allows it. Store APIs here are unofficial and break, so each store is its own
  change. Ubisoft is the first candidate.
- **User-installed manifests.** Load TOML manifests from the user's data folder, and show the launch schemes and
  paths a manifest uses before enabling it. This covers small launchers and emulators (playtime source `emulator`
  already exists).
- **Code plugins.** WebAssembly plugins behind the provider interface, with host access limited to what the plugin's
  manifest declares (paths to read, schemes to launch). Extract that API only after the bundled providers have
  settled it.
- **Heroic on Windows.** Heroic also runs on Windows; its scanner from the Linux milestone can be enabled there if
  testers ask for it.

## Deferred (not in any step)

Social features (feed, posts, reviews, journal), cloud saves, DLSS swapper, store downloads, Xbox / Microsoft Store and
Amazon (Windows) local scans, writing Steam launch options directly, controller navigation, macOS.
