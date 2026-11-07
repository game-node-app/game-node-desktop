# Spec Delta

## MODIFIED Requirements

### Requirement: Non-blocking asynchronous IPC between UI and Engine
On Linux, the application shell SHALL execute backend commands asynchronously through the per-user daemon and SHALL push daemon state updates to the frontend through coalesced backend events (`engine:snapshot`) synchronized with `@tanstack/react-query`. No file I/O, SQLite access, process inspection, network request, or image decoding SHALL run on the UI/main thread. The Windows shell SHALL retain its current backend behavior until its daemon transport is implemented.

#### Scenario: Rapid state updates while interacting with the shell
- **WHEN** the daemon emits several snapshot updates during a library scan or authentication verification
- **THEN** the Tauri host relays the newest state to the UI without blocking user input, menu navigation, or window resizing

#### Scenario: Another client changes shared state
- **WHEN** a CLI command changes daemon-owned state while the GUI is connected
- **THEN** the GUI receives the updated snapshot through its existing state bridge without requiring a restart
