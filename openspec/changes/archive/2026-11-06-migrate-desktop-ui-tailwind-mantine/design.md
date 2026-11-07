# Design: Desktop UI Tailwind CSS & Mantine v9 Migration

## Context

The desktop frontend is a Vite + React 19 SPA running inside a Tauri v2 webview (`ui/`), communicating asynchronously with a local Rust engine via Tauri IPC (`commands.*` and `@tanstack/react-query`). Following the initial port from Slint, nearly 500 occurrences of inline `style={{ ... }}` properties and bespoke modal/overlay scripts are used for component styling and dialogs.

Meanwhile, the companion `game-node-clients` monorepo establishes a mature, proven UI architecture combining Tailwind CSS 3.4 and Mantine v9, sharing identical Figma Version 3.0 design tokens (primary brand `#F15025`, dark canvas `#161616`, paper `#262525`, secondary `#1A1A1A`, and tertiary `#872C13`).

## Goals / Non-Goals

**Goals:**
- Eliminate ~496 inline `style={{ ... }}` declarations, replacing them with zero-runtime Tailwind CSS utility classes.
- Synchronize Tailwind theme configuration with `game-node-clients/packages/config-tailwind/tailwind.config.ts` (colors, spacing, typography, radii).
- Integrate Mantine v9 (`@mantine/core`, `@mantine/hooks`) for accessible, robust overlay primitives (`Modal`, `Menu`, `Tooltip`, `SegmentedControl`, `Switch`).
- Replace bespoke SVG icon files in `ui/src/assets/icons/settings/` with `@tabler/icons-react`.
- Preserve 120Hz smooth scrolling and keyboard grid navigation in `GameGrid` and `GameList` backed by `@tanstack/react-virtual`.
- Maintain full compatibility with React 19, Vite 6, and Tauri v2 window controls (`data-tauri-drag-region`).

**Non-Goals:**
- **No direct workspace dependency on `@repo/ui`**: `game-node-clients/packages/ui` is tied to `@repo/wrapper` (web API client), Next.js, and browser/mobile auth. The desktop app connects to a local Rust engine via Tauri IPC. Desktop styles will align with tokens and conventions rather than importing the web package directly.
- **No replacement of `@tanstack/react-virtual`**: Mantine layout/scroll components (`ScrollArea`, `Table`) will not replace TanStack Virtualizer in the game library.
- **No redundant Mantine wrapper components**: Avoid wrapping simple flex layouts in Mantine `Stack`/`Group` when plain Tailwind classes (`flex flex-col gap-2`) produce cleaner DOM without component overhead.

## Decisions

### 1. Tailwind CSS via PostCSS Pipeline in `ui/`
**Decision:** Configure `tailwindcss`, `postcss`, and `autoprefixer` in `ui/`, controlled by `ui/tailwind.config.ts` and `ui/postcss.config.js`.
**Rationale:** Tailwind evaluates utilities at build time with zero JavaScript runtime penalty in the WebView. It provides native support for `:hover`, `:focus-visible`, and `:active` pseudo-classes that were cumbersome to manage with inline styles.
**Alternatives considered:** CSS Modules or styled-components / emotion. Rejected because CSS Modules do not provide shared atomic design utilities, and CSS-in-JS libraries introduce runtime overhead and React 19 hydration issues.

### 2. Theme Token Alignment with `game-node-clients`
**Decision:** Extend Tailwind's theme in `ui/tailwind.config.ts` matching `game-node-clients`:
- Colors: `brand` (`#f15025` with shades 0-9), `paper` (`#161616` to `#242424`), `secondary` (`#1A1A1A`), `tertiary` (`#872C13`), `chrome` (`#111111`), `chrome-border` (`#202020`), and `text-default` (`#D9D9D9`).
- Spacing & Radii: Map to CSS variables defined in `ui/src/styles/theme.css` and Mantine theme conventions.
- Font: Roboto with system fallbacks.

### 3. Mantine v9 for Overlays and Complex Primitives
**Decision:** Install `@mantine/core` v9 and configure `MantineProvider` in `ui/src/AppProviders.tsx`.
- Configure `createTheme` with `primaryColor: 'brand'`, custom `variantColorResolver`, and dark color scheme default.
- Refactor modals (`SettingsModal`, `MatchDialog`, `AddDialog`, `DiscoveryModal`, `RemoveLocalGameModal`, `ClaimConflictModal`, `SaveProtectionModal`, `GameToolsModal`, `SyncExplanationModal`) to use Mantine `Modal` or `Dialog`.
**Rationale:** Hand-rolled modals in desktop had issues with focus trapping, outside-click bubbling, and backdrop escape keys. Mantine modals provide robust accessibility, portal mounting, and keyboard trap handling out of the box.

### 4. Tabler Icons (`@tabler/icons-react`)
**Decision:** Replace loose SVG asset files under `ui/src/assets/icons/settings/` with standardized Tabler icon components.
**Rationale:** Tabler icons match what `game-node-clients` uses, offering consistent stroke widths, sizing, and direct color inheritance via `currentColor`.

### 5. CSS Layering Order
**Decision:** Ensure proper specificity order in `ui/src/styles/theme.css`:
```css
@import '@mantine/core/styles.css';

@layer tailwind {
  @tailwind base;
}
@tailwind components;
@tailwind utilities;
```
**Rationale:** Prevents Tailwind utilities from being overridden by Mantine component default styles.

## Risks / Trade-offs

- **[Risk] CSS reset conflicts with Tauri window drag regions**: Tailwind preflight might reset draggable header elements.
  - **Mitigation:** Ensure `-webkit-app-region: drag` and `data-tauri-drag-region` are applied cleanly to `TopBar` and titlebar elements, with buttons explicitly styled with `pointer-events-auto` and `-webkit-app-region: no-drag`.
- **[Risk] React 19 peer dependency warnings with Mantine**:
  - **Mitigation:** Use Mantine v9 (`@mantine/core: ^9.0.1`), which is already tested with React 19 in `game-node-clients`.
- **[Risk] Performance regression during fast library scrolling**:
  - **Mitigation:** Keep `GameGrid` and `GameList` tile rendering lean. Do not wrap each virtual tile in Mantine components; style tile containers with Tailwind utility classes.

## Migration Plan

1. **Step 1: Pipeline & Dependencies**: Install Tailwind CSS, PostCSS, Autoprefixer, Mantine v9, and Tabler Icons; create config files and verify Vite builds.
2. **Step 2: Theme & Provider Initialization**: Setup `MantineProvider` in `AppProviders.tsx` and configure CSS layers in `theme.css`.
3. **Step 3: Shell & Navigation Refactor**: Refactor `TopBar`, `Sidebar`, `AppMenu`, and `ResizeHandle` from inline styles to Tailwind classes and Tabler icons.
4. **Step 4: Overlay & Modal Modernization**: Migrate `SettingsModal`, dialogs, and toast notifications to Mantine `Modal` and overlay primitives; deprecate `settings.css`.
5. **Step 5: Library & Feature Styling**: Refactor `LibraryToolbar`, `DetailPane`, `ContextMenu`, and `SignedOutView` to Tailwind classes.
6. **Step 6: Verification & Cleanup**: Run Vitest test suites, verify preview at `localhost:4777`, remove unused SVG assets, and verify packaging.
