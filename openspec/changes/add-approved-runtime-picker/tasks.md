# Tasks

- [x] 1.1 Add the `RuntimesRequestAccess` request/reply to the protocol and SDK under the existing
  `game.runtime.create` permission. The request carries the runtime spec without an executable
  handle plus text hints; the reply is the ordinary runtime record. Verify contract round-trips,
  generated descriptors, the required-empty executable field and that no path/handle field is
  exposed.

- [x] 1.2 Implement the host runtime approval store and handler: validate the selected regular
  executable, reject symlink escapes/directories, record identity, size and SHA-256, persist a
  durable approval keyed by plugin/publisher/profile/runtime, create the owned runtime and recheck
  it before every launch. Verify decline/cancel, idempotent retry, changed executable, revoked
  grant and profile change create or launch nothing and leak no path.

- [x] 1.3 Add installed-program suggestions. On Linux, match the official PackageKit provider's
  `executable`/`desktop_file` records against the hints; third-party providers never contribute
  candidates. Manual Browse remains. Verify matching, duplicate candidates, missing inventory and
  an unavailable PackageKit. **Windows deferral:** this change has no registry/Start Menu reader,
  so the Windows inventory is recorded for task 1.8 instead of inventing a source. The picker still
  offers manual Browse and validates the chosen program on every platform.

- [x] 1.4 Add the runtime picker host modal: requesting plugin/publisher, runtime name, suggested
  candidates (name, package, exact path), Browse, Cancel as default, en/pt-BR and keyboard/focus
  behavior. Verify the selected path is the only one approved and that cancellation returns a
  safe refusal.

- [x] 1.5 Add focused host/API/UI tests and document the request/picker flow in
  `docs/plugin-development.md` and `docs/plugin-api-permissions.md`. Verify strict OpenSpec
  validation, formatting and diff checks.

- [ ] 1.6 Implement the Dolphin launch session against the new request in the plugin repository:
  resolve the target game, get the ROM handle, request/reuse the approved runtime, bind the
  content, propose or request the recipe and start Play. Verify first-use proposal/review,
  repeated Play without a second prompt, changed runtime/content requiring fresh approval, no
  plugin-spawned process and the reference-host call sequence.

- [ ] 1.7 Run native Linux acceptance with the real Dolphin bundle and Pikmin: approve
  `/usr/bin/dolphin-emu` from the PackageKit suggestion, review the recipe, start the game, close
  Dolphin, verify the session/playtime and that a changed executable requires reapproval.

- [ ] 1.8 **Windows handoff — Lamarck (GameNode Core).** Record the registry/Start Menu suggestion
  and native picker acceptance on Windows. Linux does not depend on this handoff.

## Out of Scope

- Proton/Wine orchestration, auto-approval, plugin-supplied paths and non-emulator runtimes without
  a signed extension.
