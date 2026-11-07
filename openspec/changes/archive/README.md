# Historical archive

Documents here record past decisions and implementations. They are not instructions
for the stable application. Current requirements live in `../../specs/` and their
status policy lives in `../../README.md`.

## Discontinued UI and completed transition

Slint is discontinued. Its Skia/software renderer selection, fallback restart
flags, Advanced renderer settings, `.slint` views, gettext UI catalogs and
`ui-review` harness are archived features. They must not be restored as current
requirements. The stable UI is Tauri v2 with React and TanStack.

The following records include the discontinued implementation or its transition:

- [Foundation](2026-11-04-bootstrap-desktop-foundation/)
- [Steam library](2026-11-04-add-steam-library/)
- [Settings modal](2026-11-05-add-desktop-settings-modal/)
- [Playtime and background execution](2026-11-05-add-playtime-background/)
- [DRM-free games and backups](2026-11-05-add-drm-free-games-and-local-backups/)
- [DRM-free usability](2026-11-05-improve-drm-free-ux-and-local-backups/)
- [Per-game tools](2026-11-06-add-scalable-game-tools-and-optiscaler/)
- [Completed Tauri transition](2026-11-06-migrate-slint-to-tauri-v2-react/)
- [Completed Tailwind/Mantine transition](2026-11-06-migrate-desktop-ui-tailwind-mantine/)

Archive status applies to these records, not to all features they describe.
Authentication, discovery, playtime, backups and game tools that remain in main
specs are still current. Historical source and verification evidence are preserved.

## Superseded plugin planning

The following former `docs/mvp-roadmap.md` proposals are archived and have been
removed from its current direction. They do not define the stable architecture,
permissions, isolation guarantees or future extension contracts:

> - **User-installed manifests.** Load TOML manifests from the user's data folder, and show the launch schemes and
>   paths a manifest uses before enabling it. This covers small launchers and emulators (playtime source `emulator`
>   already exists).
> - **Code plugins.** WebAssembly plugins behind the provider interface, with host access limited to what the plugin's
>   manifest declares (paths to read, schemes to launch). Extract that API only after the bundled providers have
>   settled it.
