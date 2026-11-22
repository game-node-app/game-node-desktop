# Design

## Context

`RuntimesRegister` already accepts a runtime spec with an `executable_handle` that the host
captures into a durable `ApprovedFileReference`. The handle comes from a file approval for an
installation, save or prefix folder. That is the wrong shape for an emulator: the user should
approve one exact executable, not a folder, and the plugin must not receive a path, a folder
handle or the ability to walk next to the runtime. The Dolphin launch extension is a placeholder
exactly because this approval flow does not exist.

The provider side (`PackageKit`) already exposes a trusted installed-program record with
`executable`, `desktop_file`, `package_id` and `package_manager`, which is enough to suggest the
Dolphin executable without asking the user to browse.

## Goals / Non-Goals

**Goals**

- Let an extension ask for one approved runtime executable with a clear, reviewed picker.
- Suggest the executable from the host's installed-program inventory, with manual browse.
- Keep the path and the execution authority in the host; the extension gets an opaque runtime.
- Reuse the existing runtime, content-binding, recipe, launch-plan and supervisor contracts.
- Require fresh approval when the executable changes.

**Non-Goals**

- A new execution path for plugins; the host still launches every process.
- Approving a folder or an arbitrary plugin-supplied path.
- Proton/Wine orchestration, Windows native acceptance or emulator certification.
- Changing provider discovery/catalog behavior.

## Decisions

### D1. The extension requests; the host creates

`RuntimesRequestAccess` is a new extension request under the existing `game.runtime.create`
permission. The request carries the runtime spec **without an executable handle** and executable
hints. The host shows the picker, validates the chosen file, stores a durable runtime approval and
creates the runtime owned by the caller. The reply is the ordinary `RuntimeRecord` with an empty
`executable_handle`.

Alternative: extend file approvals with a `runtime` kind. Rejected because the file-approval model
is rooted at a folder and grants traversal of that folder; one exact executable needs its own
record and must not widen into file access. Alternative: the plugin registers with a file handle
from a prefix folder. Rejected by the user's choice and because it exposes a folder.

### D2. Request shape

- `context`: the authenticated extension session and selected-game context.
- `runtime`: name, version, platform, supported formats, argument template and optional extension
  payloads, with `executable_handle` required empty.
- `hints`: executable base names and desktop ids declared by the extension (for example
  `dolphin-emu`, `dolphin-emu.desktop`). Hints are text, never paths.
- `idempotency_key` and an optional expected revision for a retry.

The host resolves an existing owned runtime with the same name/format first; if one is valid, it
returns it without prompting. Otherwise it prompts. A declined or cancelled prompt returns a safe
refusal and creates nothing. The request is idempotent for its key.

### D3. Suggestions from the host inventory

The picker builds candidates from host-owned official sources only:

- Linux: the official PackageKit provider's `executable` and `desktop_file` records, matched
  against the hints by executable base name or desktop id. The candidate shows the display name,
  package id/manager, version when known, and the exact path.
- Windows: the installed-program inventory from the registry/Start Menu (same host-owned source
  used for local-game suggestions), matched by executable base name/display name. Native Windows
  acceptance remains a handoff. **This change has no registry/Start Menu reader yet; the Windows
  inventory is deferred to task 1.8 and the picker offers manual Browse until then.**

Third-party providers never contribute candidates. The user can always choose **Browse…** and pick
any executable; the picker shows exactly what will be approved.

### D4. Validation and durable approval

The host opens the chosen file without following a symlink escape, requires a regular executable
file (not a directory, not a script outside the platform policy), records device/node identity,
size and SHA-256, and stores a `RuntimeApproval` keyed by plugin, publisher fingerprint, profile
and runtime id. The approval carries the exact path and the digest; it grants no file read/write
scope to the plugin. The runtime record points at the approval.

Before every launch the host rechecks the approval belongs to this plugin/profile, the runtime
revision is current and the executable identity/digest still match. A changed, replaced or missing
executable invalidates the runtime and asks for a fresh picker on the next request; it never falls
back to another path.

### D5. Launch integration

The approved runtime feeds the existing `RuntimeSpec`/content binding/recipe path. The Dolphin
extension registers the runtime once, binds the selected ROM through the existing
`GamesRegisterContent`/content-location flow, and proposes or requests its recipe. The user reviews
the launch plan as today. The supervisor starts the approved executable with the reviewed
argument template and content path. No new process path is introduced.

### D6. Declines, cancellation and errors

- Picker cancelled or declined: `PERMISSION_DENIED`/`CANCELLED` with no runtime and no prompt loop.
- Selected path is not a regular executable, is a symlink escape, or fails identity checks:
  `FAILED_PRECONDITION`/`INVALID_ARGUMENT`, with no path in the status text.
- The plugin is disabled, the profile changes, the grant is revoked or the account changes while
  the picker is open: the request fails and nothing is created.
- The same idempotency key returns the same runtime; a new key after a decline starts a new
  request.

### D7. UI

The picker is a host modal, mounted with the existing approval host. It shows the requesting
plugin and publisher, the requested runtime name, the suggested candidates (name, package, exact
path), a **Browse…** action, **Cancel** as the default, and a clear statement that the selected
program will be started by GameNode for this plugin. All copy is en/pt-BR and keyboard accessible.
No path is copied into the plugin or into logs.

## Risks / Trade-offs

- **A malicious or mistaken executable** → the user sees the exact path and package; the host
  revalidates identity/hash before each launch; the plugin never handles the path.
- **Stale runtime after a package upgrade** → identity/hash change invalidates the approval and
  requires reapproval; no silent fallback.
- **Inventory changes by distro** → suggestions are advisory; manual browse always works.
- **Windows source is new** → keep it behind the same interface and record native acceptance as a
  handoff; Linux must not depend on it.
- **Prompt fatigue** → reuse a valid owned runtime and remember the approval; only changed or
  revoked executables prompt again.

## Migration Plan

1. Add the protocol/SDK request and reply with contract tests.
2. Add the host approval store, validation and handler with fixture tests.
3. Add the Linux installed-program suggestions and the Windows inventory adapter.
4. Add the picker UI and tests.
5. Implement the Dolphin launch session against the new request.
6. Run the real Linux acceptance with Dolphin and Pikmin; record Windows as a handoff.

Rollback: remove the new request and picker; existing runtimes registered through file handles and
all official launches remain unchanged.
