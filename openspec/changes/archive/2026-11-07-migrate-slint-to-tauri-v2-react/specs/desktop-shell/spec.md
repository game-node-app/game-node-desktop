# Spec Delta

## ADDED Requirements

### Requirement: Non-blocking asynchronous IPC between UI and Engine
The application shell SHALL execute all backend commands (`#[tauri::command]`) asynchronously on background worker threads and SHALL push state updates to the frontend through coalesced backend events (`engine:snapshot`) synchronized with `@tanstack/react-query`. No file I/O, SQLite access, process inspection, network request, or image decoding SHALL run on the UI/main thread.

#### Scenario: Rapid state updates while interacting with the shell
- **WHEN** the backend engine emits several snapshot updates during a library scan or authentication verification
- **THEN** the UI receives coalesced snapshot updates without blocking user input, menu navigation, or window resizing

## REMOVED Requirements

### Requirement: Renderer selection and recovery
**Reason**: Slint's Skia and software renderers (and their `--gamenode-internal-software-fallback` restart loop) are removed completely as part of the hard cutover to Tauri v2's native OS WebView compositor.
**Migration**: Rendering and hardware/software composition are managed automatically by the platform WebView runtime (WebView2 on Windows, WebKitGTK on Linux); no user-facing renderer toggle or fallback restart flag is needed.
