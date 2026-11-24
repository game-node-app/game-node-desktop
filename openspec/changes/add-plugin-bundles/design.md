# Design

## Context

GameNode installs managed plugins as single `.gnplugin` packages with one manifest, one publisher
signature, one plugin id and one kind (provider or extension). The session model requires the two
kinds to stay separate (see the archived design of `add-third-party-plugins`, decisions 8 and 11),
so a product such as the Dolphin emulator integration ships two packages. The package validator,
importer, consent review, registry and inventory all assume one package per import.

The bundle adds a distribution layer above those packages. It does not change component
registration, sessions, permissions or process supervision.

## Goals / Non-Goals

**Goals:**

- One signed file, one review, one publisher decision, one inventory entry and one uninstall.
- Reuse the existing package parser, signature, integrity, compatibility and installation code for
  every component.
- Keep per-component identity, grants, lifecycle and failure isolation.
- Fail atomically for structural problems; degrade per component for platform/API compatibility.
- Make the format reproducible and verifiable from the repository tooling.

**Non-Goals:**

- A combined provider/extension kind or one process serving both session kinds.
- Changing the `.gnplugin` format or re-signing component payloads.
- Marketplace, publishing accounts, ratings or remote catalogs.
- Changing consent semantics: the bundle approval grants no permission.
- Bundling official/bundled providers (they are not managed packages).

## Decisions

### Envelope over a multi-kind manifest

The bundle is a ZIP with `bundle.json`, `components/*.gnplugin` and one signature. The alternative
— a manifest with several components and entrypoints — would change the package schema, the
installer and the session model in one step and make a single package responsible for two process
lifecycles. The envelope keeps the existing validators as the trust boundary and lets a component
still be imported alone for development.

### Two signature layers, one publisher

`bundle.json` carries the bundle id, name, version, publisher key and, per component, its path,
SHA-256, plugin id and kind. The outer Ed25519 signature covers a domain prefix plus the exact
`bundle.json` bytes stored in the archive; the components keep their own signatures and integrity
manifests. The host verifies both layers and requires every component's publisher public key to
match the bundle's. This prevents mixing a trusted component with an untrusted one under a single
consent screen, at the cost of verifying each component twice.

Domain prefix: `GameNode/plugin-bundle/v1\0` followed by the exact `bundle.json` bytes. The pack
tool writes compact UTF-8 JSON; verification never re-serializes it.

### Atomic structural install, per-component compatibility

Staging validates the envelope, signature, component paths/hashes, each component package and
publisher equality. Any structural failure aborts before the registry changes, so there is no
half-installed bundle. Platform/API compatibility is evaluated per component: an incompatible
component is installed with an `unavailable` status while compatible siblings activate. This is
required for the motivating case (a discovery provider usable before extension supervision exists,
or on a platform without the launch target).

### Bundle metadata beside the existing registry

A migration adds two tables: `plugin_bundles` (bundle id, name, version, publisher fingerprint,
manifest hash, installed time) and `plugin_bundle_components` (bundle id, plugin id, kind,
component hash, package version). Component packages, grants and activation remain in their
existing tables and code paths. The mapping is written in the same transaction that records the
staged component installs; a failure removes staged files and leaves no mapping. Standalone
packages stay ungrouped.

Adopting an already installed component into a bundle is allowed only when its plugin id and
publisher match; otherwise the bundle import reports a conflict and asks the user to remove the
conflicting package first.

### One review, per-component decisions

The import review is a bundle DTO that embeds the existing per-package review for each component.
The UI shows one bundle header (name, version, publisher fingerprint, license aggregate) and a
grouped permission list per component; one confirmation pins the bundle publisher fingerprint.
Permission grants still go through the existing per-plugin review/replace calls, so one screen can
submit several component decisions and a denial affects only that component. No permission is
granted by importing or confirming.

### Inventory grouping without changing component identity

`PluginInfo` gains optional bundle fields (`bundle_id`, `bundle_name`, `bundle_component_kind`).
The inventory aggregates components by `bundle_id` for display: one row with the bundle name,
version, publisher and a per-component status line; enable/disable acts on the selected component.
Uninstall on the bundle row iterates its components: stop each lifecycle with the bounded grace,
remove packages, remove the mapping, and apply one retention choice. The alternative — grouping in
the UI by name/publisher — was rejected because it is heuristic and cannot drive uninstall.

### Atomic update and rollback

A bundle update stages every component, runs the existing per-package update review and activates
the whole set in one operation. The review shows each component's old/new version and permission
diff. On failure the previous bundle set stays active; rollback restores the previous set and
revalidates grants instead of widening them. Component versions remain independent; the bundle
version is the distribution version. Local bundle imports have no anti-replay sequence; the update
feed's existing signed index gains a bundle-hash field for remote updates.

### Tooling

`scripts/plugin-package.py` gains `bundle` and `bundle-verify` commands (or equivalent functions
in a sibling tool): pack the components, compute hashes, write canonical `bundle.json`, sign the
outer envelope with the pinned key, add the development-key marker, and verify the outer and inner
layers. The pack format is deterministic (fixed timestamps, sorted entries).

### Limits and validation

At most 8 components, 512 MiB outer archive, 2 GiB expanded, one nested ZIP level only. Reject
absolute or traversal paths outside `components/`, symlinks, duplicate plugin ids, case
collisions and unknown bundle schema versions. Validation never executes package content.

## Risks / Trade-offs

- **Nested-archive attack surface** → reuse the hardened ZIP reader on the outer archive with
  stricter bounds, never extract recursively, and add tamper/zip-bomb fixtures.
- **A single review hides a component change** → group by component and show old/new permission
  diffs on update; deny is per component.
- **Atomic install across files and SQLite** → stage, verify, then write files and the bundle
  mapping in one transaction with cleanup on failure; failure-injection tests.
- **Update while components run** → stop each component with the bounded grace before swapping;
  restart only those that were enabled.
- **Duplicate identities with standalone installs** → publisher-checked adoption or an explicit
  conflict; never silently overwrite.
- **Bundle support delays the Dolphin plugin distribution** → keep the two standalone packages
  working and ship the bundle as soon as this change lands; the plugin repository only adds the
  envelope.

## Migration Plan

1. Add the bundle tables in a new migration after the current latest; existing rows and packages
   are untouched.
2. Ship the host support behind the normal import UI; `.gnplugin` remains accepted.
3. Update tooling and docs; add the Dolphin bundle packaging after the host change is available.
4. Rollback: stop using bundles and keep component packages installed. The additive tables can be
   left in place; an older build refuses the newer schema as it already does for any newer
   migration.
