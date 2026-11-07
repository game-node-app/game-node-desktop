# Contributor instructions

Follow [OpenSpec documentation status](openspec/README.md) when interpreting
current requirements, proposed changes and historical records.

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
- **Styling System**: Style React views with Tailwind CSS utility classes backed by
  `ui/tailwind.config.ts` and the Version 3.0 tokens in `ui/src/styles/theme.css`.
  Do not add static inline `style` objects for layout, color, typography, borders or
  interaction states; reserve inline styles for runtime values such as virtualizer
  offsets, measured pane widths and pointer-anchored positions. Use Mantine v9
  (`Menu`, `Notification`, `Switch`, `SegmentedControl`) for complex controls; they are
  mounted through `MantineProvider` in `AppProviders.tsx`. Use the shared
  `FloatingModal` (`ui/src/components/FloatingModal`) for large work surfaces that
  benefit from dragging and resizing, and `AppModal` (`ui/src/components/AppModal`)
  for centered confirmations and notices. Both share the last-opened-on-top layer
  stack, focus trapping, Escape handling and trigger-focus restoration; do not
  hand-roll those behaviors. Use `@tabler/icons-react` instead of loose SVG assets,
  and keep `@tanstack/react-virtual` as the library virtualization engine.

## Preferred UI testing & visual verification workflow

Prefer local Vite preview (`pnpm --dir ui dev` at `http://localhost:4777`), Vitest component
tests (`pnpm --dir ui test --run`), and `cargo test --locked -p gamenode-app` over
cross-building and installing to Windows (`scripts/install-windows-wsl.sh`) when iterating on
UI layout, translations, keyboard navigation, or view-state logic.

For a signed-in UI with fictional games, use `VITE_UI_MOCK=1 pnpm --dir ui dev`.
The mock runs in memory and resets on reload. See [docs/development.md](docs/development.md)
for signed-out and locale scenarios and supported interactions. Use the browser for
isolation; the native Rust host still initializes if launched separately.

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
