# Spec Delta

## MODIFIED Requirements

### Requirement: Grid and list views
The signed-in main view SHALL show the library as a cover grid or a list, which the user can switch between. Both the grid view and the list view SHALL virtualize their visible rows using `@tanstack/react-virtual` so that only the entries inside the viewport (plus a bounded overscan) are mounted in the DOM. The list SHALL show at least the title, provider and match status. Each entry SHALL show its GameNode cover when one is cached and a titled placeholder otherwise. Cached covers and detail-pane landscape artwork SHALL be served through the custom `gamenode-asset://` protocol and rendered with asynchronous off-thread decoding (`loading="lazy" decoding="async"`) so image loading never blocks the UI thread or retains unbounded uncompressed bitmaps in Rust memory.

#### Scenario: Switch views
- **WHEN** the user switches from grid to list
- **THEN** the same entries, filter, sort and selection are shown as a virtualized list

#### Scenario: No cover available
- **WHEN** an entry is unmatched or its cover is not cached
- **THEN** it shows a placeholder with the detected title

#### Scenario: Scroll a large library with cached covers and artwork
- **WHEN** the user scrolls rapidly through hundreds of games in grid or list view or selects a game with high-resolution landscape artwork
- **THEN** only viewport-visible rows are mounted, covers and artwork decode off the UI thread via `gamenode-asset://`, and scrolling remains responsive without unbounded memory growth
