# Proposal

## Why

A launch recipe is authored and signed by the plugin. The host can show its runtime and
arguments and let the user choose among approved recipes, but the user cannot adjust emulator
flags, per-game workarounds or other launch options without a plugin update. Editing raw argv
would bypass the signed recipe, so the host must expose only reviewed, bounded options.

## What Changes

- **Plugin-declared launch options.** An extension declares an ordered set of launch options in
  its signed manifest/configuration schema: stable id, label, description, type
  (boolean/enum/text/bounded integer), default, allowed values or bounds, and whether the option
  is plugin-wide or can be overridden per game.
- **Host-owned editor and storage.** The host renders the options in Settings › Plugins
  (Configure) and in the game's launch method, stores values per profile/plugin and an optional
  per-game override, and passes them to the plugin through the existing configuration API.
- **Recipe mapping stays plugin-owned.** The plugin maps the chosen values to literal recipe
  arguments or environment entries. The host validates the resulting recipe and shows the full
  command in the launch-plan review; an option cannot select another executable, URI, shell,
  content or working directory outside the approved runtime.
- **Changes require review.** Changing an option invalidates the saved selection/approved plan;
  the next Play shows the updated plan. Reset to defaults is explicit and also requires review.
- **Useful summary.** The launch-method selector shows the runtime/version and the concrete
  arguments after applying the options.

## Capabilities

### Modified Capabilities

- `desktop-external-plugins`: declared launch options, validation, per-game overrides and
  reapproval.
- `desktop-settings`: launch-option editor and reset flow.

## Impact

- Extension manifest/configuration schema and SDK convenience types.
- Host configuration storage, merge order (defaults → plugin-wide → per-game) and the launch-plan
  review.
- UI in plugin details and the launch-method section; en/pt-BR.
- Dolphin launch extension as the first consumer (extra flags, video backend).
- No change to runtime/content approvals or official providers.

## Out of Scope

- Raw host-side argv/environment editing, shell commands and arbitrary executable/URI selection.
- A central recipe marketplace; plugin updates still deliver new option schemas.
- Windows native acceptance (a separate handoff; the option schema is platform-neutral and may
  declare platform-specific values).

## Status

Deferred by the user after recording. Not started; no implementation is part of this change yet.
