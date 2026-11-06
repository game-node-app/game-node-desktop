# Contributor instructions

- NEVER block the UI thread. Run blocking I/O, image decoding and resizing,
  expensive computation, and waits on background workers or asynchronous tasks.
  Async tasks must offload blocking work to a worker thread. Keep UI callbacks
  and rendering updates short; send completed results back to the UI thread.
- Follow the commit-message convention in [CONTRIBUTING.md](CONTRIBUTING.md).
  It applies to AI-generated commit messages as well as human-authored ones.
- Choose the commit type by intent and the scope by affected area. Distinguish
  application changes from documentation, repository tooling, builds, and CI.
- Keep unrelated changes separate and preserve existing contributor work.
- These conventions do not authorize committing, pushing, or rewriting history;
  perform those actions only when requested.

## Preferred UI testing & visual verification workflow

Always prefer [`scripts/dev-screen.sh`](scripts/dev-screen.sh) (backed by
[`crates/app/examples/ui-review.rs`](crates/app/examples/ui-review.rs)) and
`cargo test --locked -p gamenode-app` over cross-building and installing to
Windows (`scripts/install-windows-wsl.sh`) when iterating on UI layout,
translations, keyboard navigation, or view-state logic.

### How to run from scratch

1. **Start the live screen server + hot-reload watcher**:
   ```sh
   ./scripts/dev-screen.sh start
   ```
   - Compiles `ui-review` (`--locked`), launches the headless Slint software
     renderer on `http://127.0.0.1:4777`, keeps the latest frame synced at
     `/tmp/gamenode-screen.png`, and watches `crates/app/ui/**`,
     `crates/app/examples/ui-review.rs`, and `i18n/**` in the background.
   - Editing any `.slint`, `.po`, or `ui-review.rs` file triggers an automatic
     incremental rebuild and hot-reloads the server while preserving the active
     screen preset, locale, and window dimensions via `/tmp/gamenode-ui-state.json`.

2. **Inspect and interact (CLI or T3 Code Preview)**:
   - **CLI actions** (each command updates the live server, writes
     `/tmp/gamenode-screen.png`, and prints the current UI state JSON):
     ```sh
     ./scripts/dev-screen.sh status                         # Print current screen & property state JSON
     ./scripts/dev-screen.sh preset <name> [en|pt_BR] [WxH] # Jump to a preset (see list below)
     ./scripts/dev-screen.sh locale pt_BR                   # Switch locale (en or pt_BR)
     ./scripts/dev-screen.sh resize 640x420                 # Resize window (e.g. 1100x720, 640x420, 1440x900)
     ./scripts/dev-screen.sh click 520 340 [left|right]     # Click at logical (x, y)
     ./scripts/dev-screen.sh drag 220 300 300 300           # Drag from (x1, y1) to (x2, y2)
     ./scripts/dev-screen.sh scroll 550 360 0 -160          # Scroll wheel (dx, dy) at (x, y)
     ./scripts/dev-screen.sh key Escape                     # Send key (Escape, Tab, Return, UpArrow, DownArrow, F5, ...)
     ./scripts/dev-screen.sh type "Portal"                  # Type text into focused input
     ./scripts/dev-screen.sh shot /tmp/my-shot.png          # Save a PNG or PPM screenshot
     ./scripts/dev-screen.sh stop                           # Stop background server and file watcher
     ```
   - **Available presets**:
     - Screens: `signed-out`, `library` (`grid`), `list`, `drm-detail`,
       `drm-missing`, `detail-long`, `settings`
     - Modals: `protection`, `restore`, `discovery`, `editor`, `claim`,
       `match`, `add`, `add-summary`, `sync-explanation`
     - States: `banner`, `empty`, `nothing-matches`
   - **Browser / T3 Code Preview (`preview_*` tools)**:
     Open `http://127.0.0.1:4777` in a browser or via `preview_open`
     (`target: { kind: "environment-port", port: 4777 }`) when interactive
     browser preview is requested.
   - **One-shot snapshot mode (no server)**:
     ```sh
     cargo run --locked -p gamenode-app --example ui-review -- /tmp/shot.png library protection pt_BR
     ```

3. **Run automated headless UI & translation tests**:
   ```sh
   cargo test --locked -p gamenode-app --test library_screen --test library_pt_br --test i18n_catalogs
   ```

### Limitations (when native OS testing is still required)

`ui-review` and `scripts/dev-screen.sh` render the exact `.slint` components,
fonts, theme tokens, and `.po` translations using Slint's deterministic
`MinimalSoftwareWindow` against in-memory fixture state. However, they **do not**
replace native OS acceptance for:
- Real OS window-manager chrome, DPI scaling, multi-monitor positioning, or
  Windows system-tray (`crates/app/ui/tray.slint`) behavior.
- Skia GPU renderer initialization and automatic GPU-to-software fallback.
- Real browser OAuth sign-in, single-instance IPC forwarding, and custom URI
  scheme (`gamenode://`) activation.
- OS-specific adapters in `gamenode-platform`: Windows Credential Manager /
  Linux Secret Service, native file/folder pickers, Windows PE header and
  process/parent tracking, directory-watch notifications, or Ludusavi subprocess
  execution.

For those OS-level checks, use `cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked`
and `scripts/install-windows-wsl.sh` as documented in
[docs/development.md](docs/development.md).
