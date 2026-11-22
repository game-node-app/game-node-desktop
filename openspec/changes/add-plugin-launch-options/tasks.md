# Tasks

Status: **deferred by the user after recording.** Not started; nothing below is implemented.

- [ ] 1.1 Define the signed launch-option schema and SDK convenience types: stable id, label,
  description, boolean/enum/text/bounded-integer type, default, allowed values/bounds, plugin or
  per-game scope, optional platforms and impact text. Validate duplicate ids, bounds, control
  characters and unsupported types; reject paths, handles, executable/URI fields and shell text.
  Verify a plugin without options is unchanged.

- [ ] 1.2 Store effective values per profile/plugin and per-game overrides, merge in the order
  default → plugin-wide → per-game, and pass the effective map through the existing plugin
  configuration API when a recipe is proposed or reused. Reject unknown ids and stale profile or
  plugin-version writes; keep the previous values on failure.

- [ ] 1.3 Add the launch-option editor to Settings › Plugins (Configure) and the selected game's
  launch method, with per-game override and explicit reset to defaults. Show the plugin's impact
  text and the effective values; keep en/pt-BR, keyboard and focus behavior. A changed value must
  mark the saved selection/approved plan stale and trigger the normal launch-plan review.

- [ ] 1.4 Add the Dolphin launch extension's options (for example extra flags and a video backend)
  and map them to literal recipe arguments/environment. Verify the runtime/executable/content stay
  the approved ones, the summary shows the resulting command and no option can select another
  executable or URI.

- [ ] 1.5 Cover schema/bounds, merge precedence, unknown ids, reset and review invalidation in
  focused core/app/UI tests; document the author and user flow; run the full affected suites,
  formatting, Clippy and strict OpenSpec validation. Record native Linux acceptance and a Windows
  handoff for Lamarck.

## Out of Scope

- Raw host-side argv/environment editing, shell commands, arbitrary executables/URIs and a central
  recipe marketplace. Windows native acceptance is a handoff.
