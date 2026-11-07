# Providers

A provider is a game source: a store, a launcher or approved manual registrations. Each one is a TOML
manifest in `providers/` at the repository root. Manifests are embedded in the application at
build time by `crates/app/build.rs`. The application never reads a manifest from disk at run time
and never loads one from a user-writable location.

A manifest is validated when the application starts. An invalid manifest is skipped and logged to
stderr with the file name, the provider id (when it can be read) and the problem. The other
providers still load. Set `RUST_LOG=info` to see more detail from scans.

## Schema

Unknown fields are rejected. The URI-provider fields are illustrated below using the bundled Steam
manifest as the example:

```toml
id = "steam"
name = "Steam"
platforms = ["windows", "linux"]
source = "steam"
scan = { native = "steam" }
launch = { uri = "steam://rungameid/{id}" }
install = { uri = "steam://nav/games/details/{id}" }
match = { external_category = 1 }
exclude_ids = ["228980", "1628350", "1493710"]

[[roots.windows]]
registry = 'HKCU\Software\Valve\Steam'
value = "SteamPath"

[[roots.linux]]
path = "~/.steam/steam"
[[roots.linux]]
path = "~/.local/share/Steam"
```

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Stable id: lowercase letters, digits, `-`, `_`. Used in stored entry keys. Must be unique. |
| `name` | yes | Name shown in the sidebar and in error messages. |
| `platforms` | yes | `windows`, `linux`. A manifest that does not list the current platform is not loaded and is not shown as a filter. |
| `source` | yes | The store or source name, including the playtime source. |
| `scan` | yes | `{ native = "<name>" }`. Only scanners built into the application exist (`steam` and `manual`). An unknown name rejects the manifest. |
| `launch`, `install` | no | URI providers use `{ uri = "<template>" }`. The bundled manual provider additionally supports `launch = { executable = "manual" }`; it has no Install action. |
| `match` | no | `{ external_category = N }`: the GameNode external-game category of the provider's ids. This enables store-id lookup. DRM-free entries instead support automatic matching from conservative local evidence; see `desktop-game-matching` in the main specs. |
| `exclude_ids` | no | Provider ids that are never shown: tools, runtimes and redistributables. |
| `roots.windows`, `roots.linux` | no | Where to look. `path` (a leading `~` is the home folder) or `registry` plus `value`. |

Registry roots are only valid under `roots.windows` and must start with `HKCU\`. A root that does
not exist is not an error: the scan simply finds nothing there. Roots that resolve to the same
folder (for example a symbolic link) are scanned once.

## Records

A scan produces records: a provider-local `id`, an optional `name`, an `install_dir`, and further
named text fields (Steam adds `state_flags`). A record without a usable `id` or `install_dir` is
dropped and logged, and the rest of the scan continues. Repeated ids keep the first record.

## Launch and install templates

For URI providers, Play and Install open a URI built from the template. The bundled manual provider instead launches an approved executable as described below; it has no Install action.

- Only `{name}` placeholders with names made of lowercase letters and `_` are allowed. Placeholders
  can refer to `id`, `name`, `install_dir` or any other field of the record.
- The template must be a URI with a literal scheme followed by `://`. A placeholder in the scheme
  rejects the manifest.
- Values are percent-encoded: everything except letters, digits, `-`, `.`, `_` and `~` is
  encoded, so a value cannot add a path segment, query, fragment, user or another scheme.
- A value with a control character is refused.
- If the record lacks a field the template needs, Play or Install is unavailable for that game
  and nothing is opened.
- The filled URI must keep the template's scheme. It is handed to the operating system as one
  argument, never through a shell.

For example, `steam://rungameid/{id}` with id `620` opens `steam://rungameid/620`.

## The exclude list

Steam stores runtimes, Proton builds and redistributables as apps next to games. They are listed in
`exclude_ids` so they never show up as games. The list is data, so a new Proton build is a one-line
change in `providers/steam.toml`. It is checked against a real Steam install with Proton during
acceptance.

## Steam scanner

For each root the scanner reads `steamapps/libraryfolders.vdf` (falling back to
`config/libraryfolders.vdf`; both the current and the older format), always includes the root
itself, and reads every `steamapps/appmanifest_*.acf` in each library folder. A game is kept when
its manifest has a numeric app id and `steamapps/common/<installdir>` exists. An unreadable or
malformed file, or a library folder that is not available (an unplugged drive), is skipped and
logged. Its games leave the library view but keep their saved matches.

The bundled Steam Install action opens the game’s Steam library page, where Steam exposes its
installation controls. This also gives a visible destination for an already-installed game.

## Playtime counters

An optional top-level `playtime = { counter = "steam" }` selects a built-in store
counter. Unknown counters invalidate only that manifest. Steam declares `steam`;
its fields and read schedule are documented in `steam-playtime-counter.md`.
The global Store preference uses that counter; Internal uses measured sessions.
Providers without a supported counter always use measured sessions.

## DRM-free registrations (Windows)

The bundled manual provider reads approved Windows-user registrations rather than
searching a store. Pending discovery candidates and removed registrations are
excluded. An unavailable executable retains its entry and offers Locate executable;
there is no Install action or store playtime counter. Provider-local UUIDs remain
stable after a title correction, GameNode rematch or Locate operation. Measured
sessions use source `manual`, platform 6.

```toml
id = "manual"
name = "DRM-free"
platforms = ["windows"]
source = "manual"
scan = { native = "manual" }
launch = { executable = "manual" }
```

The executable declaration is reserved for this bundled provider. Other providers
use URI templates. The application reads the approved executable, working directory
and argument vector from local storage and revalidates the target before launching
on a worker. Manifest text cannot supply executable commands. Windows file identity
prevents a replaced target from silently receiving approval. No shell interprets the
stored arguments. Linux omits the manual provider.
