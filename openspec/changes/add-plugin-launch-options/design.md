# Design

## Context

The signed recipe is the execution contract. The host can already show its `LaunchRecipeOption`
summary, select a recipe and review the resolved launch plan. The plugin configuration UI exists
for plugin-owned settings, but the Dolphin launch extension exposes no launch options and the
recipe arguments are fixed in the signed binary.

Users need small, predictable adjustments (extra Dolphin flags, a video backend, a per-game
workaround) without a plugin update, while the host must still review the exact command.

## Goals / Non-Goals

**Goals**

- Let plugins declare safe, bounded launch options once and expose them to the user.
- Support plugin-wide defaults and per-game overrides.
- Keep the executable, URI, content binding and working directory under the existing approved
  runtime/recipe rules.
- Show the resulting command and require a fresh review when an option changes.

**Non-Goals**

- Raw argv or environment editing in the host, shell parsing or executable/URI overrides.
- New wire RPCs: the existing plugin configuration API carries the values.
- Automatic option migration between plugin versions (new defaults apply; changed meaning needs
  a new option id).
- Windows native acceptance in this change.

## Decisions

### D1. Declarative options, not raw argv

The extension declares options in its signed configuration/launch schema. The host renders form
controls and validates types/bounds; the plugin maps values to a recipe. A raw host editor is
rejected because it would let the host construct arguments the plugin never reviewed and would
make every plugin's command line a host-maintained surface.

### D2. Schema and scope

Each option has:

- `id` (stable, namespaced by the plugin), `label`, `description`, `type`;
- `default`, optional `allowed_values` (enum), `min`/`max`/`max_length` (integer/text);
- `scope`: `plugin` (one value for every game) or `game` (overridable per library entry);
- optional `platforms` so a plugin can present Windows-only or Linux-only choices;
- optional `required` and an `impact` note for the review.

Values are text, booleans, enum ids or bounded integers. No option carries a path, a handle, an
executable, a URI or shell text. A plugin that declares no options keeps today's behavior.

### D3. Storage and merge order

The host stores the plugin-wide values in the plugin/account configuration and the per-game values
in a small launch-option record keyed by profile, plugin, entry and option id. The effective value
is default → plugin-wide → per-game. The plugin reads the effective map through the existing
configuration API when it proposes or reuses a recipe. The host does not interpret plugin-specific
values.

### D4. Review and invalidation

The resulting recipe is still saved through `GamesProposeCustomLaunch` and resolved through the
launch-plan review. The summary shows the runtime/version and the effective literal arguments.
Changing an option marks the saved selection/approved fingerprint stale; the next Play shows the
updated plan and requires approval. Reset to defaults clears the overrides and also invalidates
the approval.

### D5. Safety limits

The host enforces the declared type and bounds, rejects control characters and values outside the
allowed set, and caps the number/size of options and per-game overrides. Recipe and launch-plan
validation remain the final boundary: a value that produces an invalid recipe or plan is refused
with an actionable message, and the previous approval is preserved.

### D6. Compatibility

Options are additive. New plugin versions may add options with new defaults; removing or changing
an option id is a manifest change covered by the normal update review. Existing recipes without
options continue to launch unchanged. Platform-specific values are data; the host never branches
on a store or provider.

## Risks / Trade-offs

- **Plugin options can change emulator behavior** → the launch-plan review shows the exact
  command; the user approves; no silent change.
- **Per-game overrides drift** → a reset action and the entry summary show the effective values;
  the saved plan is invalidated on change.
- **Too many options** → schema caps and a compact editor; advanced options stay in plugin
  configuration.
- **Old plugin versions** → unknown option ids are ignored by the host and not sent to the plugin;
  the plugin keeps its defaults.

## Migration Plan

1. Add the schema/SDK types and manifest validation.
2. Add host storage, merge and configuration pass-through.
3. Add the plugin-details and per-game editors with en/pt-BR and reset.
4. Add Dolphin options and map them to its recipe.
5. Test schema bounds, merge precedence, invalid values and review invalidation; record native
   Linux acceptance and a Windows handoff.
