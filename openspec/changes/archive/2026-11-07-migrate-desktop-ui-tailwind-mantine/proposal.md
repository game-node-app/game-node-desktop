# Proposal: Migrate Desktop UI to Tailwind CSS and Mantine v9 Primitives

## Why

The recent cutover from Slint to Tauri v2 and React established a high-performance web frontend, but left nearly 500 inline `style={{ ... }}` blocks, manual CSS stylesheets, and hand-rolled overlay/focus logic spread across UI components. At the same time, the companion `game-node-clients` monorepo already standardizes on Tailwind CSS 3.4 and Mantine v9 with identical Figma Version 3.0 design tokens (brand `#F15025`, dark background `#161616`, secondary `#1A1A1A`, and tertiary `#872C13`). Migrating `game-node-desktop` to Tailwind CSS and selective Mantine overlay primitives eliminates inline styling debt, improves UI maintainability, standardizes accessible modal/focus behavior, and aligns developer experience with the broader GameNode ecosystem without compromising desktop virtualization performance.

## What Changes

- **Add Tailwind CSS & PostCSS Pipeline**: Install and configure `tailwindcss`, `postcss`, and `autoprefixer` in `ui/`, mirroring the design tokens, color palette, font sizes, and radii from `game-node-clients/packages/config-tailwind`.
- **Integrate Mantine v9 Overlays & Theme**: Install `@mantine/core` (v9) and `@mantine/hooks`, configuring `MantineProvider` in `AppProviders.tsx` with the dark theme override and variant color resolver matching GameNode branding.
- **Replace Ad-hoc SVGs with Tabler Icons**: Install `@tabler/icons-react` and replace bespoke SVG asset files in `ui/src/assets/icons/settings/` with standardized Tabler icon components.
- **Adopt Accessible Overlay Primitives**: Refactor modals and dialogs (`SettingsModal`, `MatchDialog`, `AddDialog`, `DiscoveryModal`, `RemoveLocalGameModal`, `ClaimConflictModal`, `SaveProtectionModal`, `GameToolsModal`, `SyncExplanationModal`, `DiscardChangesDialog`) to leverage Mantine `Modal` and overlay primitives with robust focus trapping, portal isolation, and escape handling.
- **Refactor Inline Styles to Tailwind Utilities**: Incrementally migrate ~496 inline `style={{ ... }}` declarations across `shell`, `library`, `settings`, `drm-free`, and `auth` into idiomatic, zero-runtime Tailwind utility classes.
- **Preserve Desktop Virtualization & Custom Chrome**: Preserve `@tanstack/react-virtual` for `GameGrid` and `GameList`, custom window dragging (`data-tauri-drag-region`), and Tauri IPC snapshot architecture without regression.

## Capabilities

### Modified Capabilities
- `desktop-shell`: Update theme implementation and overlay requirements to specify Tailwind CSS token alignment, Mantine v9 accessible overlay primitives, and Tabler icons while preserving desktop performance and keyboard accessibility.

## Impact

- **UI Dependencies**: Adds `tailwindcss`, `postcss`, `autoprefixer`, `@mantine/core`, `@mantine/hooks`, and `@tabler/icons-react` to `ui/package.json`.
- **Styling Architecture**: Consolidates custom CSS and inline styles into Tailwind utility classes; replaces `settings.css` and custom modal positioning with Mantine modals and Tailwind classes.
- **Developer Workflow**: Vite HMR and build pipeline continue to compile to `ui/dist` for Tauri v2 bundling; Vitest component tests and integration tests continue to run via `pnpm --dir ui test`.
