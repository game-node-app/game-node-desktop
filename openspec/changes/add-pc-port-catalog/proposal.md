# Proposal

## Why

Community native PC ports and decompilation projects, such as Lantern Engine for Bloodborne, let people play games they already own on Windows. Setting one up is still a long manual job: find the right project, pick a safe release, find the right dump or ROM version, extract a PS4 PKG with a separate tool, put the files where the port expects them, and add the result to a launcher by hand. GameNode already registers, launches, tracks and backs up DRM-free games, so it can take over the rest of that work. It never distributes game files: the person always supplies their own copy.

No open, licensed manifest of these ports exists. Quiver Launcher publishes a public, read-only catalog API with about 1,200 entries. It includes verified releases, checksums, withdrawn releases, and machine-readable setup recipes for the files a player supplies. GameNode will use that API as its upstream without contacting Quiver. It will credit Quiver as text on the catalog screen, and it will keep working from its local cache if the API changes or blocks access. The product owner accepts that risk.

## What Changes

- **Explore destination in the sidebar.** A new `EXPLORE` group sits under `YOUR SPACE` and contains one destination, **PC ports**. Selecting it replaces the library in the content area with the port catalog. Selecting All games or a provider returns to the library. Exactly one item is active at a time. Port installs that are running show a badge on PC ports. The application still opens on the library.
- **Port catalog.** The host fetches the upstream catalog in the background, caches it locally, and refreshes it at most daily or when the user asks. The catalog view lists every Windows-compatible entry in a virtualized grid and list. It has search, filters (source console, guided setup, verified release, installed), and a detail pane with description, project, licence, the files the user must supply, the release that will be installed, and the source repository. The screen shows "Port catalog by quiverlauncher.com" in text, with a link and no Quiver logo or icon.
- **GameNode overlay.** A GameNode-maintained overlay, keyed by the upstream's stable `catalogId`, adds or corrects what the upstream lacks: the GameNode game id, the launcher and gameplay executables, setup recipes (for example the CUSA03173 v1.09 dump for Lantern Engine), extraction steps, edition definitions, and a hide flag. A copy of the overlay is bundled with the app, and a newer copy can be fetched from this repository.
- **Whole catalog installs.** Every listed Windows entry can be installed. The install downloads the verified release over HTTPS, checks its SHA-256, extracts it safely into a folder the user chooses, applies the entry's `filesToAdd`, and registers the preferred executable as a DRM-free library entry. Entries that have setup files, from the upstream or the overlay, get a **guided setup**. It asks for the user's own file or folder, checks it against accepted hashes or marker files, and places it or passes it as the recipe says. Entries without a recipe install the release and leave first-run setup to the port.
- **Dump extraction.** When the overlay says a setup file can come from a console package (for example a PS4 `.pkg` base game plus its update), GameNode downloads a pinned extraction tool from GitHub, verifies it, and runs it without a shell to produce the folder the port needs. GameNode distributes no game content and no keys.
- **Release safety.** Only releases the upstream marks as verified are offered by default. Withdrawn releases are never installed. Files whose upstream checksum changed after verification, or that a scan flagged, need explicit confirmation. Pre-release-only projects are labelled as such.
- **Library integration.** Installed ports appear in the library as DRM-free entries, with launcher and gameplay roles, playtime tracking, save backups and automatic GameNode matching from the overlay's GameNode id. Removing an entry never deletes installed files. Ports that update themselves (for example Lantern Engine) are not overwritten by GameNode. They keep their registration, and repair works if their executable moves.
- **Long work stays off the UI thread.** Downloads, hashing, extraction and copying large dumps run in host workers. They report progress, can be cancelled, survive closing the window, and leave no half-registered entries after a crash.

**Out of scope:** Linux installs (Windows first, Linux deferred), installing mods, submitting reviews or problem reports to Quiver, installing unverified releases by default, automatic port updates, deleting installed files, and any distribution of game data, keys or firmware.

## Capabilities

### New Capabilities

- `desktop-port-catalog`: upstream catalog fetch and cache, GameNode overlay merge, offline and degraded behaviour, the PC ports browsing surface (grid, list, search, filters, detail pane), and on-screen credit.
- `desktop-port-install`: release selection and verification, install folder choice, safe extraction, guided setup of user-supplied files and folders, pinned extraction tools, install jobs with progress, cancellation and crash recovery, and registration of the installed port in the library.

### Modified Capabilities

- `desktop-shell`: the signed-in sidebar gains the `EXPLORE` group with the PC ports destination, single-active-destination navigation, an install badge and keyboard access.
- `desktop-drm-free-games`: DRM-free entries can be created by a port install. They carry port provenance and arguments built from the setup recipe, and they handle repair and removal as other DRM-free entries do.
- `desktop-local-state`: persist the catalog cache, overlay version, install jobs, port provenance and the last chosen install folder through a new ordered migration.
- `desktop-settings`: the Tools panel also lists the helper extraction tools, with licence, source, cached versions and Clear cache.
- `desktop-game-matching`: a GameNode id supplied by the port overlay counts as evidenced identity for automatic matching.

## Impact

- **Core (`crates/core`)**: new `ports` domain: catalog model, overlay schema and merge, setup-recipe evaluation, install plan, job state machine, and ports for the catalog source and the installer. The view model gains `PortsView`.
- **Platform (`crates/platform`)**: HTTP client for the upstream API written from its public OpenAPI description (no Quiver code or types are copied), catalog cache, downloads with SHA-256 checks, archive extraction reusing the game-tools path validation, an extraction-tool runner that uses no shell, large file copies, and migration `0008_ports.sql`.
- **App (`crates/app`)**: async Tauri commands for catalog refresh, install, cancel and pick paths. The `engine:snapshot` payload gains catalog state, and a separate event channel carries job progress.
- **UI (`ui/src`)**: sidebar `EXPLORE` group, `features/ports` (catalog grid and list, toolbar, detail pane, guided setup `FloatingModal`), i18n in English and Brazilian Portuguese, mock host fixtures, Vitest coverage.
- **Data**: new `ports/overlay.toml` in the repository, bundled at build time and fetched from `main` at runtime.
- **External dependency**: `api.quiverlauncher.com` (unauthenticated, read-only GET requests only), GitHub release downloads for ports and extraction tools.
- **Docs**: new `docs/pc-ports.md` covering the catalog source and credit, overlay authoring, guided setup, extraction and the deferred Linux scope.
