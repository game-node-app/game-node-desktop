# PC ports

**PC ports** installs community native PC ports and decompilation projects that run on
Windows. The catalog is browsable on a signed-in Windows build. GameNode downloads the
port project's own release, asks for the game files the port needs, places or extracts
them as the recipe says, and registers the result as an ordinary DRM-free library entry.
GameNode never supplies game content, keys or firmware.

Linux installs are deferred; see [Linux scope](#linux-scope).

## Catalog source and credit

The catalog is read from the public Quiver Catalog API at
`https://api.quiverlauncher.com/api/v1`. The client lives in
`crates/platform/src/ports/quiver.rs` and is built from the public OpenAPI description,
not from Quiver's code. It uses unauthenticated, read-only requests only:

- `/apps` is paged at `limit=100` with `nextCursor`, up to 200 pages.
- `/apps/{slug}` and `/apps/{slug}/releases` are read for a detail pane and, together
  with the entry's withdrawn releases, when an install starts.

Every request carries
`User-Agent: GameNode-Desktop/<version> (+https://github.com/game-node-app/game-node-desktop)`.
No authorization header, cookie, account id, device id or GameNode token is sent.
HTTP 429 is retried after `Retry-After` when that is at most 60 seconds; a longer
cool-down is reported as rate limiting instead of stalling the worker. Without
`Retry-After`, the client backs off exponentially, and a request is attempted at most
four times. No POST, report, review or other write endpoint is ever called.

Upstream responses are mapped into GameNode's own catalog model. Unknown fields are
ignored. Pages are parsed item by item; an item missing `catalogId`, `slug` or `name` is
skipped and counted. More than 20 % skipped (exactly 20 % is accepted), an empty listing,
or a listing that ends before its last page fails the whole refresh, so a breaking
upstream change is reported instead of half-applied.

PC ports shows the text **Port catalog by quiverlauncher.com** in the catalog view and in
the detail pane. It is a link that opens `https://quiverlauncher.com` in the system
browser; it is translated, while entry names, descriptions and recipe labels are shown as
received. There is no Quiver logo or icon, and nothing suggests that Quiver endorses
GameNode. Quiver's packages are "All rights reserved", and its trademark policy allows
truthful text references but not its logo. Each entry also credits its port project and
author as the upstream names them.

### Accepted API risk

The product owner accepts that the upstream API may change shape, add authentication or
block GameNode. There is no agreement with Quiver and no support channel; GameNode only
reads the public endpoints it documents. If the API stops working, catalog refreshes fail;
the local cache and the GameNode overlay keep working, and installed ports never depend on
the API. We do not mirror or re-host the catalog, its artwork, its releases or its game
data.

### Cache, refresh and offline behaviour

- The last complete listing is stored in the `port_catalog_snapshot` table and shown
  immediately on later visits. See [local-state.md](local-state.md) for the stored fields.
- The first fetch runs when PC ports is first opened. A snapshot older than 24 hours is
  refreshed in the background, at most once a day and at most one attempt per day while
  refreshes keep failing. **Refresh** starts one on demand and is coalesced into a refresh
  that is already running.
- A new snapshot replaces the stored one only after the source returned a complete,
  validated listing. A failed refresh keeps the previous snapshot and reports the stale
  state; with no usable snapshot, PC ports shows the unavailable state with **Retry**.
- The catalog status is `idle`, `refreshing`, `ready`, `stale` or `unavailable`, and the
  header shows when the cached copy was last updated.
- Entry details and releases are not part of the listing. They are fetched when the detail
  pane needs them or an install starts, and reused in memory for 15 minutes. Installing a
  port whose release list cannot be loaded does not start and reports that the release
  information is unavailable.
- An installed port is an ordinary DRM-free entry. It launches, tracks playtime and backs
  up saves offline with no catalog present.

## The GameNode overlay

`ports/overlay.toml` in this repository corrects and extends the upstream catalog, keyed by
the upstream's stable `catalogId`. The copy in the repository is embedded into the
application at build time with `include_str!`, so PC ports always has a valid overlay,
including on the first run and offline. The host may fetch a newer copy from
`https://raw.githubusercontent.com/game-node-app/game-node-desktop/main/ports/overlay.toml`
with an ETag. The fetch runs when PC ports is opened and on **Refresh**, one request at a
time, with redirects disabled, a 60-second timeout and a 1 MiB document cap.

A copy that fails to parse or validate, or that declares a newer major `schema_version`,
is ignored whole. The newest valid copy already stored (or the bundled copy) stays in use,
and an accepted overlay is re-merged into the cached catalog without another fetch. A
failed copy therefore leaves the previous overlay in full effect.

### Merge rules

- An overlay scalar field replaces the upstream value when present; an omitted field keeps
  the upstream value.
- An overlay `setup_file` list replaces the whole upstream `setupFiles` list, because
  partial merging of recipe slots is too easy to get wrong. `setup_file = []` removes
  guided setup.
- `hidden = true` removes the entry from the list. An existing installation stays in the
  library and keeps its provenance.
- `launcher` becomes the DRM-free registration's executable and launcher; `gameplay`
  becomes its gameplay executables.
- Only entries that support Windows are listed, including entries without a recipe.
- The overlay cannot introduce an entry the upstream does not list, and it cannot change
  the recipe's `folderName`, `filesToAdd`, `releaseAssetFilter`, `preferredExecutables`,
  description, artwork or supported operating systems. Those come from the upstream.
- Extractor pins are derived from the package slots of the current overlay. A re-merge
  never keeps a pin the overlay no longer declares.

Unknown fields are ignored by the parser, so a misspelt field silently does nothing. Only
the errors listed under [Validation](#validation) reject an overlay.

### Schema

Top level:

| Field | Required | Meaning |
| --- | --- | --- |
| `schema_version` | yes | Major version. This build accepts `1`; missing (`0`) or newer versions are rejected whole. |
| `extractor` | no | `[[extractor]]` tables for the tools that package slots use. |
| `entry` | no | Entries keyed by the upstream `catalogId` (`[entry."qcat_..."]`). |

`[[extractor]]` pins one extraction tool:

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Unique id referenced by `from_package.extractor`. |
| `name` | yes | Name shown in the interface. |
| `repository` | yes | `owner/repo` on GitHub; a `https://github.com/` prefix is also accepted. |
| `tag` | yes | The release tag, such as `v1.2.3`. |
| `asset` | yes | The release asset file name. |
| `sha256` | yes | 64 hexadecimal characters for the asset. Lowercase by convention. |
| `license` | yes | The tool's licence, shown with the helper tools. The loader does not reject an empty value, but author it. |
| `executable` | yes | The executable inside the extracted tool folder, as a safe relative path. |
| `args` | yes | Argument template. It may be empty in the schema; in practice it must point the tool at `{out}` and at the packages, or output validation fails. |

`[entry."<catalogId>"]` corrects one upstream entry:

| Field | Required | Meaning |
| --- | --- | --- |
| `gamenode_game_id` | no | GameNode game id used as matching evidence. `0` is the "not authored yet" placeholder and is treated as absent. |
| `launcher` | no | Launch executable relative to the install folder. It becomes both the executable and the launcher. |
| `gameplay` | no | Gameplay executables relative to the install folder. `[]` clears the upstream list. |
| `license` | no | The port project's licence. The upstream listing has no licence field. |
| `release` | no | A release version to install instead of the upstream verified one. It must match a release `version` exactly. |
| `edition` | no | An edition note carried on the merged entry. The current detail pane does not render it yet. |
| `hidden` | no | `true` removes the entry from the list. Defaults to `false`. |
| `setup_file` | no | Replaces the upstream setup-file list. |

`[[entry."<catalogId>".setup_file]]` describes one file or folder the player supplies:

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | Unique within the entry. |
| `label` | no | Name shown in the interface; defaults to the id. |
| `prompt` | no | Extra instruction shown with the slot. |
| `kind` | no | `file` or `folder`. Defaults to `folder` when `contains` or `from_package` is present, else `file`. |
| `required` | no | Defaults to `true`. An optional slot can be skipped. |
| `destination` | no | Relative path under `root` where an accepted choice is copied. |
| `root` | no | `app` (the port's folder), `data` (`%APPDATA%`), `config` (`%APPDATA%`) or `localData` (`%LOCALAPPDATA%`). Defaults to `app`; only applies with `destination`. |
| `flag` | no | Pass the choice's path to the port after this flag. A flag ending in `=` joins the path (`--iso=<path>`). |
| `positional` | no | Pass the choice's path as an argument of its own, before flags. Defaults to `false`. |
| `env` | no | Environment variable the launch receives with the choice's path. |
| `contains` | no | Files a folder must contain, such as `eboot.bin` or `sce_sys/param.sfo`. Markers must name files. |
| `extensions` | no | File-picker suffixes, without a dot. |
| `accepted` | no | Any one of these hashes accepts the choice: a table with `algo` (`sha256` or `sha1`), `value` (hex) and `label` (the dump's name). |
| `hash_file` | no | Folder file the accepted hashes apply to. Defaults to the first `contains` entry. |
| `from_package` | no | `{ extractor = "<id>", title_id = "...", needs_update = "..." }` lets console packages produce the folder. |

A slot uses exactly one of `destination`, `flag`, `positional` and `env`. With none of
them the port asks for the choice itself; GameNode only validates it and passes nothing.
The user's original file or folder is never moved or modified.

`from_package.extractor` is required. `title_id` and `needs_update` are shown to the user.
The install plan requires an update package when `needs_update` is set; GameNode does not
read package metadata to check the title or version. The slot's `contains` markers and
accepted hashes decide whether the extracted folder is the right dump.

### Validation

An overlay is rejected whole when:

- the document is not valid TOML;
- `schema_version` is missing or newer than the build supports;
- an extractor id is declared twice, or an extractor is missing a required field, has a
  SHA-256 that is not 64 hexadecimal characters, or has an executable that is not a safe
  relative path;
- an entry has an empty catalog id, or a `launcher` or `gameplay` path that is not a safe
  relative path;
- a setup slot has no id, more than one placement, an unsafe `destination`, `contains` or
  `hash_file` path, an invalid environment variable name, an accepted hash of the wrong
  length or with non-hex characters, `from_package` on a `file` slot, an empty or
  undeclared `from_package.extractor`, or a duplicate slot id.

A safe relative path uses `/` or `\`, is not absolute and has no drive prefix, `.` or `..`
component, control character or empty component. Unsafe paths are refused before anything
is written.

Two things are checked later instead of at parse time:

- An `args` template is expanded before the tool runs. `{package}`, `{update}` and `{out}`
  must each be an argument of their own; any other braces reject the template.
  `{update}` needs an update package.
- A folder slot with accepted hashes and neither `hash_file` nor `contains` parses, but no
  choice can be accepted because there is no file to hash.

An overlay entry whose catalog id matches no upstream entry is valid and unused.

### Example

The block below is an example, not real catalog data. The catalog id, game, tool, digests
and title id are fictional. It shows an entry with the scalar corrections, a required
game-folder slot, an accepted hash and a package option.

<!-- overlay-example -->

```toml
# Example only: this entry, tool and every digest below are fictional.
schema_version = 1

[[extractor]]
id = "example-pkg-tool"
name = "Example PKG extractor"
repository = "example-owner/example-pkg-tool"
tag = "v1.0.0"
asset = "example-pkg-tool-win64.zip"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
license = "MIT"
executable = "example-pkg-tool.exe"
# This fictional tool merges the update itself; a tool without {update} is run twice by GameNode.
args = ["extract", "{package}", "--update", "{update}", "--out", "{out}"]

[entry."qcat_example_port"]
gamenode_game_id = 0
launcher = "Example Game Launcher.exe"
gameplay = ["Example Game.exe"]
license = "GPL-3.0-or-later"
release = "v1.2.3"
edition = "Example edition note."

[[entry."qcat_example_port".setup_file]]
id = "game"
label = "Example Game (EXAMPLE0000) with update 01.00"
kind = "folder"
required = true
contains = ["eboot.bin", "sce_sys/param.sfo"]
hash_file = "eboot.bin"
accepted = [
  { algo = "sha256", value = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789", label = "EXAMPLE0000 01.00" },
]
destination = "EXAMPLE0000"
from_package = { extractor = "example-pkg-tool", title_id = "EXAMPLE0000", needs_update = "01.00" }
```

The example is a test fixture. The test `the_documented_overlay_example_parses_and_validates`
in `crates/core/src/ports/overlay.rs` reads this file with `include_str!`, extracts the
first TOML block after the `<!-- overlay-example -->` marker, and parses and validates it
through the real overlay loader. Run it with:

```sh
cargo test --locked -p gamenode-core the_documented_overlay_example_parses_and_validates
```

### Lantern Engine: how it finds the game folder

These findings come from Lantern Engine's source at `v0.9.39-beta.31`. Windows acceptance
is task 10.1.

| Finding | Value |
| --- | --- |
| Mechanism | A folder picker on the launcher's Play page, with a detected default. |
| Stored in | `game_dir` in `%APPDATA%\Lantern Engine\settings.json`. |
| Detected default | `<parent of the package folder>\CUSA03173`, used while `game_dir` is empty. |
| Package folder | `BloodbornePC` inside the install folder, because the release archive nests everything there. |
| Resulting recipe | Launch `BloodbornePC/Lantern Engine.exe` and copy the game folder to `CUSA03173` in the install folder. |

The recipe needs no `config_write` step, because the copy lands where Lantern Engine looks
first. The folder name does not limit the edition: Lantern Engine reads the title from
`sce_sys/param.sfo`.

The recipe lists no hashes. Lantern Engine's launcher accepts several retail title ids at
version 01.09, and its engine compares the game image at every start, so it decides what it
can run. GameNode only checks that the folder has `eboot.bin` and `sce_sys/param.sfo`.

### The pinned PS4 PKG extractor (task 1.2)

The overlay pins [`paulomanrique/ps4-pkg-extractor`](https://github.com/paulomanrique/ps4-pkg-extractor).
It is a small command-line tool with an open licence that runs on Windows and fails with a
clear message for an encrypted retail package.

| Finding | Value |
| --- | --- |
| Repository (`owner/repo`) | `paulomanrique/ps4-pkg-extractor` |
| Tag | `v0.0.1` |
| Asset | `pkgextract-x86_64-pc-windows-msvc.zip` |
| SHA-256 | `0c736ee02779fb5cf4403a91da23f94fdaa72debf995be0c400ac9c36acee63a` |
| Licence | Unlicense |
| Executable inside the tool folder | `pkgextract.exe`, at the archive root |
| Argument template | `["{package}", "-o", "{out}", "--overwrite"]` |

The tool's usage is `pkgextract <package.pkg> -o <folder>`. It takes **one package per
run** and has no update or merge option. It also refuses an output folder that already
exists unless `--overwrite` is passed; the runner pre-creates an empty output folder, so
the template must keep that flag. Those facts were checked against a base and update
package from a dump the developer owns on Windows:

- the base package produced 28,830 files (31.5 GB);
- the update package produced a partial tree of 442 files: 423 replace files in the base
  and 19 are new, such as `sce_discmap_patch.plt` and `sce_sys/changeinfo/*`;
- merging every update file over the base at the same relative path, without deleting
  anything, produced 28,849 files, and `sce_sys/param.sfo` reports version 01.09
  afterwards. Lantern Engine accepts that version and decides for itself which retail
  title ids it can run.

Because the tool merges nothing, GameNode runs it twice when a slot has an update
package: the base extracts into the slot's output folder and the update into a temporary
folder beside it, inside the staging folder and on the same volume. GameNode then moves
each update file over the base. Nothing in the base is deleted, and the base is never
copied. Validation runs on the merged folder only, because the update tree alone does not
carry the slot's markers. A tool whose template contains `{update}` is run once instead,
because it merges the update itself.

`ports/overlay.toml` declares this extractor and gives the Lantern Engine `game` slot a
`from_package` option with `needs_update = "01.09"` and no `title_id`; the folder option
still works. The loader, download, hash check, two-run extraction, merge and runner
described below are implemented and tested. The Windows acceptance of the merged folder
by Lantern Engine itself remains task 10.1.

## Guided setup and installation

### Before the job starts

**Install** opens the guided setup surface. It confirms the release, asks for the parent
folder (the last one used is offered first), shows one slot for every file or folder the
merged recipe asks for, and lets a package-capable slot switch between a user-picked
folder and a base package plus, when the recipe requires it, an update package. A slot
that has a package option opens on the package choice; **Extracted folder** is the other
position of the same control, for a game folder that is already unpacked. Required slots
block the install; optional slots can be skipped. The modal can be closed while the job
keeps running.

When the user starts, the host loads the entry detail and release list, merges the overlay
again and plans the install. Planning is pure and every failure happens before a job is
queued:

- The entry must support Windows.
- The release is the overlay's pinned version when it names one, else the upstream
  verified version. A withdrawn release is refused with its reason, and a pinned version
  that no longer exists is refused as unavailable.
- The Windows asset is chosen by the recipe's `releaseAssetFilter`, then by operating
  system, then by architecture: x86-64 first, unknown next, anything else last. An asset
  without a declared operating system is not assumed to be a Windows one. Only `zip`, `7z`
  and a bare `.exe` are installable; an installer-only entry is listed but Install is
  unavailable.
- The file needs a checksum. A file whose verified checksum changed upstream, or that a
  malware scan flagged, needs the explicit confirmations the modal asks for. With no
  checksum at all, the install cannot be planned.
- The install subfolder is named from the recipe's `folderName`, else the entry name,
  else the slug, sanitized for Windows. It must be absent or empty, must not be a
  registered game folder or sit inside one, and the volume must have room for the
  download, twice its size as an extraction estimate, the supplied files, and twice the
  size of a package slot's base and update packages as its extraction estimate. A plan
  that refuses one of these rules is reported with a translated reason, not a raw error.
- Every `filesToAdd` path must be safe, and every required slot must have a choice. A
  choice for an optional slot that disappeared between picking and starting is skipped.

The plan also fixes the launch arguments: positional paths first, then flags (a joined
flag keeps its value attached), and the user's own arguments follow them later. `env`
slots become the port's launch environment.

### Running an install

One worker thread runs every install; jobs queue behind it, because hashing and copying
large dumps in parallel only causes disk contention. The phases are:

```text
Queued -> Resolving -> Downloading -> Verifying -> Extracting
       -> SuppliedFiles(slot i: Hashing | Extracting package | Copying)
       -> Finalizing -> Registering -> Completed
       -> Finalizing -> AwaitingExecutable -> Registering -> Completed
  any unfinished -> Cancelling -> Cancelled
  any unfinished -> Failed(reason)
  any unfinished -> Interrupted(reason)   (startup recovery, after cleanup)
```

All work for `<parent>\<folder>` happens in `<parent>\.gamenode-staging-<job id>`, on the
target volume. Every path the job creates under the install folder, the staging folder or
an external root is journaled before it is written. Finalizing is one directory rename, so
nothing is ever half visible at the final path.

The release asset is downloaded to `<data_dir>/ports/cache/<catalogId>/<version>/` and
checked against its SHA-256 before extraction. A mismatch deletes the file, extracts
nothing and fails the job. A verified cached file is reused by a retry.

The archive is extracted in-process with the shared archive code: absolute paths, parent
traversal, links that escape the folder and a total extracted size above 16 GiB are
rejected. A bare `.exe` asset is copied into the folder as is. `filesToAdd` entries are
created as empty files when extraction did not produce them.

Progress is reported on the `ports:progress` event, coalesced to at most four updates per
second. `engine:snapshot` carries phase changes and results, not progress ticks. The
worker keeps running while the window is hidden.

### Supplied files

A file is read once, with SHA-1 and SHA-256 computed in the same pass, with progress and
cancellation between chunks of at most 8 MiB. A folder must contain every `contains`
marker as a file; its accepted hashes apply to `hash_file`, or to the first marker when
the overlay does not name one. Without accepted hashes, a choice's contents are not
rejected beyond the folder markers above. A choice that fails the hash check names the
accepted versions. When a package-capable slot is filled in folder mode and the chosen
folder directly holds `.pkg` files, the rejection points at the slot's package option
instead of only naming the missing marker. The folder scan and every hash run on the
install worker, never on the UI thread.

An accepted file or folder is copied, never moved:

- `destination` under `app` is copied into the staging folder during the supplied-files
  phase;
- `destination` under `data`, `config` or `localData` is copied during Finalizing, through
  a temporary name that is renamed when complete;
- copies are written under a temporary name, the copy is re-hashed and compared with what
  was accepted, then renamed into place;
- `flag`, `positional` and `env` slots copy nothing. The plan already contains the path,
  flag or environment value.

A failed copy removes what it wrote.

### Console packages

When the slot declares `from_package` and the user chooses packages, the pinned tool is
prepared under `<data_dir>/ports/tools/<id>/<tag>/`: its GitHub release asset is
downloaded, checked against the overlay SHA-256 and extracted into a sibling `tool/`
folder. A checksum mismatch fails before a process is created. When the overlay sets
`from_package.needs_update`, the install plan refuses a choice that has only the base
package; the modal states which update is required.

The tool runs without a shell, with no console window and inside a Windows job object (a
process group elsewhere) so cancelling or quitting kills its process tree. Its working
directory is the staging folder. Its arguments come only from the overlay template:
`{package}`, `{update}` and `{out}` are replaced with canonicalized paths, each as its own
argument, and no other braces are allowed. Combined stdout and stderr keep the last
64 KiB for the failure details. If the output looks like an encrypted retail package, the
job fails with a message that GameNode ships no keys.

A tool that takes one package per run is run twice when the slot has an update. The base
extracts straight into the slot's output folder. The update extracts into
`.gamenode-update-<slot id>` in the staging folder, never inside the game folder, and
GameNode then overlays it: every update file replaces or joins the base at the same
relative path, new folders are created, and nothing is deleted or copied except the few
update files themselves. The temporary folder is removed after the merge and on any
failure. The merged folder is validated once with exactly the same marker and hash rules
as a user-supplied folder; the two intermediate trees are not validated, because either
one alone may lack the slot's markers. A template that contains `{update}` keeps the
single-run behaviour for a tool that merges the update itself.

Both extracted trees are treated as untrusted. A symbolic link, a reparse point, or any
path that would resolve outside the game folder fails the job instead of being merged,
and cancellation is checked between entries.

A slot that copies into the port folder is extracted straight to its destination inside
the staging folder, so a multi-gigabyte dump is not copied twice. A slot passed by flag,
positional argument or environment variable is extracted to
`.gamenode-slot-<slot id>` inside the port folder and that final path is passed to the
port.

### Awaiting an executable and registration

After Finalizing, the installer looks for the executable the registration will launch: the
user's explicit pick, else the overlay `launcher` or the first upstream preferred Windows
executable, then the remaining launchers. Each candidate must be a safe relative path and
an existing file inside the install folder.

If none exists, the job enters `AwaitingExecutable`, keeps its complete folder and leaves
the worker free for the next job. The interface asks the user to pick an `.exe` inside the
install folder. The pick must be a relative `.exe` path; its links are resolved and must
stay inside the folder. A rejected pick leaves the job waiting for another one. On the
next start, recovery keeps such a job and the runner registers it again, so the pick or a
cancel still works after a restart.

An overlay `launcher` replaces the upstream preferred list as well as the executable, so a
missing overlay launcher is not retried against the upstream names; the install waits for
the user's pick instead.

At the commit point the host reads the executable's volume and file identity, builds the
DRM-free `Registration` (title, absolute executable, install folder as root and working
directory, recipe arguments, launchers and gameplay), and writes the registration, the
provenance row and the completed job in one SQLite transaction. Only then does the job
count as installed.

### Cancel, failure and restart recovery

Cancellation is a shared flag checked between chunks of at most 8 MiB and, for a running
extractor, a kill of its process tree. The job moves through `Cancelling` to `Cancelled`,
removes every journaled path newest first plus its staging folder, and registers nothing.
A failure does the same and records the reason and the capped tool output for the failure
details. Verified cached downloads are never journaled and survive both.

Quitting during a job ends it as `Interrupted` with a reason. At the next start, recovery
runs before the install worker:

- an unfinished job that waits for an executable pick keeps its complete folder and phase;
- a job whose registration was committed is marked completed and keeps its entry,
  provenance and files;
- any other unfinished job has its journaled paths and staging folder removed, keeps its
  cached downloads, and is marked `Interrupted` with **Retry**.

**Retry** reuses the stored plan with a new job id and staging folder, so it does not need
to re-plan or re-ask for the provided files. The setup window therefore does not show the
slot pickers for a retry; start a new install to use different files.

### Library entry, provenance and launch

The installed port appears in the library as a DRM-free entry. Port provenance lives
beside the registration in `port_installations`: registration id, catalog id, slug, port
name, installed version, install folder, environment map and job id. It has no foreign key
to the registration table, so **Remove from library** keeps the provenance row and every
installed file; if the same executable is added again, the provenance is relinked.

On launch, the environment map from provenance is added to the inherited environment and
the registration's arguments are used as stored: recipe arguments first, then the user's
own. The install folder is the game root and working directory.

After a completed install, the overlay's `gamenode_game_id` is passed to the local matcher
as the strongest kind of evidence. Without an overlay id, the upstream game title is
passed as title evidence, and the usual conservative rules decide whether to match. A
choice the user made still wins.

## Safety rules

- **Verified releases only by default.** Install uses the release the upstream verified,
  or the overlay's pinned release. A withdrawn release is never installed; the detail pane
  shows the withdrawal reason. A pre-release-only entry still installs its verified
  pre-release and is labelled as such. An entry with no verified release shows why Install
  is unavailable.
- **Changed checksums need confirmation.** A file whose verified checksum changed upstream
  has only an unverified checksum; the install asks first and then checks the download
  against that checksum. A scan-flagged file asks first and names the engine count.
- **Every download is verified before use.** Release assets and extraction tools are
  fetched over HTTPS and checked against their SHA-256. A mismatch deletes the file,
  extracts nothing and fails the job.
- **Extraction tools are pinned.** A tool is fixed by tag, asset name and SHA-256 in the
  overlay. A hash mismatch fails before any process starts. Updating a pin is an overlay
  change, not an application release.
- **Archives are extracted defensively.** Absolute paths, parent traversal, links that
  escape the folder and an extracted size above 16 GiB are rejected. A failed extraction
  removes what it wrote.
- **The install folder is checked.** It must be new or empty, must not be a registered
  game folder or sit inside one, and must have room for the download, its extraction
  estimate and the supplied files.
- **No content distribution.** GameNode downloads only the port project's own release and
  a pinned extraction helper. It ships no game data, keys, firmware or decryption
  material. Guided setup requires the user's own file or folder and validates it against
  the recipe's markers and known hashes when it has them; GameNode never downloads,
  searches for or suggests a source of game content. An encrypted retail package is
  reported and not extracted.
- **No upstream writes.** Install, cancel and failure never send a report, review, check
  or other write request to the catalog upstream.

## Self-updating ports

Some ports, including Lantern Engine, update themselves through their own launcher.
GameNode does not update, overwrite, reverify or repair the files of an installed port
after the install completes. A port that replaces its own files keeps its registration and
library entry, and GameNode never restores the files it changed.

When the catalog's verified release is newer than the installed version, the detail pane
says an update is available and links to the project page. It does not reinstall over the
install folder, and there is no automatic port update. Use the project's own updater, then
keep playing the same library entry.

The DRM-free repair flow still applies: if the executable moved, **Locate executable** in
Game settings repairs the registration without changing its identity. The host also
records the launch executable's file identity at install time and rechecks it before
launch. If a self-update replaces that executable file, the launch is refused as a changed
target and **Locate executable** repairs it. Files overwritten in place normally keep
their identity and launch normally.

## Linux scope

Windows comes first; Linux installs are deferred.

- `PortPlatform::supports_installs()` is true only on Windows. Linux and macOS builds do
  not start the PC ports service, and the `EXPLORE` destination is hidden on non-Windows
  signed-in builds (`?platform=linux` in the UI mock shows that state).
- PC ports lists only upstream entries that support Windows. Linux-only entries are not
  shown.
- The install engine is Windows-specific by design: `%APPDATA%` and `%LOCALAPPDATA%`
  placement roots, Windows file identity for launch validation, `.exe` executables,
  `CREATE_NO_WINDOW`, job objects and a pinned Windows extraction tool.
- Supporting Linux installs would need Linux asset selection, placement roots, launcher
  validation, extractor pins and a Linux extraction tool, plus a review of the overlay
  recipes. Nothing in this change promises that work.

## Implementation notes and spec divergences

These points record where the code behaves differently from the change specs, or where a
detail needs a qualification:

- **Spec divergence: a failing release response does not remove the entry.** The listing
  snapshot has no releases; they are fetched lazily. An entry whose detail or release
  request fails stays listed, and its detail pane or Install reports that the release
  information is unavailable. The catalog spec's scenario that omits such an entry is not
  how the lazy design can behave.
- **Spec divergence: a folder's accepted hashes apply to one file.** The spec says a
  folder's "marker hashes" match; the code hashes exactly one file, `hash_file` or the
  first `contains` entry, with both SHA-1 and SHA-256.
- **Spec divergence: a replaced self-updated executable needs repair.** The spec scenario
  says the library entry still launches after a port updates itself. The code rechecks the
  executable's file identity at launch, so a replacement is refused until **Locate
  executable** repairs the registration. Rewriting the file in place normally keeps
  working.
- **Spec divergence: a second installation has no catalog action.** The spec says
  installing the same entry into another folder creates a separate installation. The data
  model allows it (each job has its own registration and provenance), but once an entry is
  installed its detail pane offers **Open in library**, not a second Install.
- **Implementation note: the overlay's `edition` note is not displayed yet.** It is parsed,
  validated and merged onto the entry, but the current view model and detail pane do not
  render it.
- **Implementation note: `title_id` and `needs_update` are display and gating data.** The
  host does not read package metadata to verify them. `needs_update` is enforced while the
  install is planned: a base-only package choice is refused. The extracted folder is checked
  like any supplied folder: its `contains` markers, and accepted hashes when the recipe
  lists them. Without hashes, the port decides whether it can run that dump.
