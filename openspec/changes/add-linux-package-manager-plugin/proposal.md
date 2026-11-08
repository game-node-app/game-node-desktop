# Proposal

## Why

Linux users need an additional way to discover games installed from distribution packages, including games such as SuperTux installed through pacman. The earlier scanner implementation is not usable with the current host because its discovery RPC is disabled; adapting it to the public provider SDK will deliver package discovery without creating a parallel plugin protocol.

## What Changes

- Add an official Linux-only provider plugin that identifies game candidates from installed desktop entries and reports them through the current provider SDK using `provider.context.read` and `library.discovery`.
- Select pacman ownership queries on Arch-family distributions and PackageKit queries elsewhere, preserving stable provider records and scan-failure behavior.
- Register the plugin and its grants in the trusted host inventory; describe its native file, process, and D-Bus access separately from API capabilities.
- Build and place the plugin beside the desktop executable in Linux development and release packages. Keep it out of Windows builds and packages.
- Keep package discovery independent from Steam account, catalog, login, playtime, and PC ports behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `desktop-providers`: define Linux package-manager discovery, backend selection, provider record behavior, and failure preservation.
- `desktop-settings`: require the Linux package-manager plugin to appear in the selectable plugin inventory with its source, runtime status, API grants, and native access.

## Impact

The workspace gains a Linux-only plugin executable and focused scanner fixtures. The Tauri host gains a trusted package-provider descriptor and discovery routing; the bundled provider manifest and selectable plugin inventory expose it on Linux. Linux development/release scripts and package verification include the plugin, while Windows packaging explicitly excludes it. The public protocol remains unchanged and the Windows app continues to build without a package-manager executable.
