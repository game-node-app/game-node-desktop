# Tasks

## 1. Build Pipeline & Styling Dependencies

- [ ] 1.1 Add `tailwindcss`, `postcss`, `autoprefixer`, `@mantine/core`, `@mantine/hooks`, and `@tabler/icons-react` to `ui/package.json` and verify `pnpm --dir ui install` succeeds
- [ ] 1.2 Create `ui/postcss.config.js` and `ui/tailwind.config.ts` matching tokens from `game-node-clients` (brand, paper, secondary, tertiary, radii, spacing) and verify build configuration resolves correctly
- [ ] 1.3 Import `@mantine/core/styles.css` and `@tailwind` directives in `ui/src/styles/theme.css` and configure `MantineProvider` in `ui/src/AppProviders.tsx`; verify `pnpm --dir ui test --run` succeeds

## 2. Shell & Navigation Refactor

- [ ] 2.1 Refactor `TopBar` and `AppMenu` from inline styles to Tailwind utility classes and Tabler icons, ensuring `data-tauri-drag-region` remains draggable and `pnpm --dir ui test --run src/features/shell` passes
- [ ] 2.2 Refactor `Sidebar` and `ResizeHandle` from inline styles to Tailwind utility classes, preserving `@tanstack/react-store` responsive resize logic and keyboard focus rings
- [ ] 2.3 Refactor `SignedOutView` and `NoticeBanner` in `ui/src/features/auth` from inline styles to Tailwind utility classes and verify `pnpm --dir ui test --run src/features/auth` passes

## 3. Overlay Dialogs & Settings Modernization

- [ ] 3.1 Refactor `SettingsModal`, sidebar tabs, and settings panels (`AccountPanel`, `StoresPanel`, `ToolsPanel`, `BackupsPanel`, `AboutPanel`, `LicenseNoticesModal`, `DiscardChangesDialog`) to Mantine `Modal` and Tailwind utilities, replacing `settings.css` and ad-hoc SVGs; verify `SettingsModal.test.tsx` passes
- [ ] 3.2 Refactor `ToastContainer` and notification toasts to Mantine primitives and Tailwind utilities, verifying dismiss and auto-close behavior
- [ ] 3.3 Refactor DRM-free and backup dialogs (`DiscoveryModal`, `LocalGameEditor`, `RemoveLocalGameModal`, `ClaimConflictModal`, `SaveProtectionModal`) to Mantine `Modal` and Tailwind classes; verify `drmFree.test.tsx` passes
- [ ] 3.4 Refactor shell dialogs (`SyncExplanationModal`, `MatchDialog`, `AddDialog`, `GameToolsModal`) to Mantine `Modal` and Tailwind classes; verify `dialogs.test.tsx` and `GameToolsModal.test.tsx` pass

## 4. Library Views & Detail Pane Refactor

- [ ] 4.1 Refactor `LibraryToolbar` to Tailwind utility classes and Tabler icons, preserving search input, view-mode switches, and filter counts
- [ ] 4.2 Refactor `DetailPane` to Tailwind utility classes and Tabler icons, preserving artwork background, action buttons, and playtime formatting
- [ ] 4.3 Refactor `ContextMenu` to Mantine `Menu` / overlay primitives and Tailwind classes, verifying right-click positioning and keyboard dismissal
- [ ] 4.4 Refactor `GameGrid` and `GameList` tile/row markup to Tailwind classes, preserving `@tanstack/react-virtual` virtualization arithmetic and keyboard focus navigation; verify `library.test.tsx` passes

## 5. Cleanup & End-to-End Verification

- [ ] 5.1 Remove deprecated `ui/src/features/settings/settings.css` and obsolete loose SVG assets under `ui/src/assets/icons/settings/`
- [ ] 5.2 Run complete frontend verification (`pnpm --dir ui test --run` and `pnpm --dir ui build`) ensuring clean compilation and asset bundling for Tauri v2
- [ ] 5.3 Update documentation in `AGENTS.md` and `docs/development.md` documenting Tailwind CSS and Mantine usage conventions
