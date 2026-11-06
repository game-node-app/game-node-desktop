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

## Architecture Guardrails (Tauri v2 + React + TanStack)

- **Async Tauri Commands**: All Rust commands exposed to the frontend must be
  asynchronous `#[tauri::command]` handlers that never block the main thread.
  Offload compute or blocking I/O to worker threads or Tokio tasks.
- **Custom Asset Protocol**: Cover images and artwork are served over the custom
  `gamenode-asset://` protocol (`gamenode-asset://localhost/cover/<file>` and
  `gamenode-asset://localhost/artwork/<game_id>`). Always render covers in React using
  `<img loading="lazy" decoding="async" />`.
- **Virtualized Views**: The game library grid and list must use `@tanstack/react-virtual`
  to maintain high rendering performance with large game collections.
- **State & Query Bridge**: Use `@tanstack/react-query` to bridge engine snapshot events
  and query cache updates, and `@tanstack/react-store` for responsive UI state (e.g., sidebar resizing).

## Preferred UI testing & visual verification workflow

Prefer local Vite preview (`pnpm --dir ui dev` at `http://localhost:4777`), Vitest component
tests (`pnpm --dir ui test --run`), and `cargo test --locked -p gamenode-app` over
cross-building and installing to Windows (`scripts/install-windows-wsl.sh`) when iterating on
UI layout, translations, keyboard navigation, or view-state logic.

### How to run from scratch

1. **Start the Vite development server**:
   ```sh
   pnpm --dir ui dev
   ```
   - Serves the React frontend on `http://localhost:4777` with fast HMR.
   - Watches `ui/src/**` and recompiles instantly on edit.

2. **Inspect and interact via T3 Code Preview (`preview_*` tools)**:
   - Use `preview_open` with target port `4777` (`http://localhost:4777`) to preview and interact
     with the application interface in real time.
   - Run Vitest tests in `ui/` for automated verification:
     ```sh
     pnpm --dir ui test --run
     ```

3. **Run automated Rust tests**:
   ```sh
   cargo test --locked -p gamenode-app
   ```

### Limitations (when native OS testing is still required)

Vite preview renders the web interface with in-memory fixture state or mocked IPC.
However, it **does not** replace native OS acceptance for:
- Real OS window-manager chrome, DPI scaling, multi-monitor positioning, or native
  Windows system-tray behavior.
- Real browser OAuth sign-in, single-instance IPC forwarding, and custom URI
  scheme (`gamenode://`) activation.
- OS-specific adapters in `gamenode-platform`: Windows Credential Manager /
  Linux Secret Service, native file/folder pickers, Windows PE header and
  process/parent tracking, directory-watch notifications, or Ludusavi subprocess
  execution.

For those OS-level checks, use `cargo xwin test --workspace --target x86_64-pc-windows-msvc --locked`
and `scripts/install-windows-wsl.sh` as documented in
[docs/development.md](docs/development.md).
