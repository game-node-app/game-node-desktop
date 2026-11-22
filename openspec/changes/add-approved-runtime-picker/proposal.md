# Proposal

## Why

The Dolphin bundle discovers, matches and catalogs GameCube/Wii games, but **Play** still says the
store cannot launch through GameNode. The launch extension is a placeholder and the host has no
way for the user to approve an emulator executable. A plugin cannot spawn a process, and approving
a whole folder is too broad for a runtime: the host needs one exact, user-confirmed executable.

## What Changes

- **Extension-initiated runtime access.** An extension asks the host for an approved runtime and
  supplies the runtime spec (name, version, supported formats, argument template, platform) plus
  executable hints (names/desktop ids). The host shows a picker and the user approves one exact
  executable.
- **Host-owned runtime.** The host validates the selected file (regular file, executable,
  non-escaping path, identity and SHA-256), stores a durable runtime approval, and creates the
  runtime for the plugin. The extension receives the runtime record/id, never a path or file
  handle. A changed executable invalidates the runtime and requires reapproval.
- **Installed-program suggestions.** The picker suggests candidates from the host-owned inventory:
  PackageKit records on Linux, the registry/Start Menu program list on Windows. Manual browse is
  always available. Third-party providers cannot inject candidates.
- **Play through the existing launch plan.** The approved runtime feeds the existing recipe,
  review and supervisor path; the plugin proposes or reuses the recipe, and the host launches the
  approved executable with the reviewed arguments and content path.
- **Dolphin launch session.** The Dolphin extension implements the target context, runtime request,
  ROM handle, content binding and recipe propose/request flow, so its **Play** starts Dolphin.

## Capabilities

### Modified Capabilities

- `desktop-plugin-api`: new `RuntimesRequestAccess` under the existing `game.runtime.create`
  permission; replies carry a runtime record and no executable path.
- `desktop-external-plugins`: approved runtime selection, suggestions and reapproval rules.

## Impact

- Desktop: protocol/SDK message and helper, host runtime approval store and handler, installed-program
  suggestion source, approval UI (en/pt-BR), contract/host/UI tests.
- Dolphin plugin repository: launch session implementation and its reference-host tests.
- Docs: plugin development and permission guides; runtime troubleshooting.
- No change to provider discovery, matching, Steam/PackageKit discovery, official launches or the
  existing recipe/launch-plan contracts.

## Out of Scope

- Proton/Wine orchestration, Windows native acceptance (Lamarck handoff) and emulator certification.
- Auto-approval, plugin-supplied executable paths outside the host picker, and non-emulator runtimes
  without a signed extension.
- Changing the provider API or letting a provider launch a game.
