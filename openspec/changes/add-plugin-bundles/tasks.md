# Tasks

## 1. Bundle format and tooling

- [x] 1.1 Define the bundle schema and signature domain (`GameNode/plugin-bundle/v1\0` plus the exact `bundle.json` bytes) in the platform/package code, and verify a fixture bundle parses with the expected id, version, publisher and component hashes.
- [x] 1.2 Extend `scripts/plugin-package.py` (or add a sibling tool) with `bundle`/`bundle-verify`: canonical `bundle.json`, sorted deterministic ZIP entries, component SHA-256, outer signature and the development-key marker. Verify a round trip succeeds and a one-byte component change fails.
- [x] 1.3 Add bundle format docs to `docs/plugin-package-manifest.md` and `docs/plugin-package-registry.md` with the file layout, signature domain, component paths and limits, and verify every documented command runs as written.

## 2. Host bundle validation

- [x] 2.1 Implement the bundle verifier: ZIP bounds, `bundle.json` parsing, outer signature, component path/hash checks, nested-bundle rejection and component-publisher equality. Verify tamper, missing component, traversal, duplicate id and mixed-publisher fixtures all fail before registry writes.
  - Completion: `verify_bundle_archive`/`verify_bundle_bytes` in `crates/platform/src/plugin_bundles.rs` validate the outer ZIP entries and bounds, exact metadata names, nested/unlisted rejection, canonical `bundle.json`, the outer signature, every component SHA-256 and each component's signed integrity (same publisher key, matching plugin id and kind). Tests cover a valid bundle, tampered and missing components, duplicate ids, mixed publishers, identity mismatch, a component declaring another key, unexpected entries and a nested bundle. 10 module tests pass; platform Clippy is clean.
- [x] 2.2 Enforce the bundle limits (component count, outer/expanded size, one nested level, case collisions) in the verifier and tooling. Verify an oversized or zip-bomb fixture is rejected with an actionable error.
  - Completion: the verifier enforces the component count, outer and expanded size, per-entry read bounds, one nested level, case collisions and metadata sizes; the tool enforces the outer compressed limit, expansion limit and entry count. Tests cover case-colliding and excess entries, bounded reads over their limit, and the tooling's expansion/compressed limits against a real bundle. 12 module tests and 7 tooling tests pass.
- [x] 2.3 Verify each component with the existing package validator and compatibility evaluator instead of a parallel implementation, and record per-component verdicts in the bundle review. Verify a corrupt component and a platform-incompatible component produce different outcomes.
  - Completion: `inspect_bundle_candidate(archive, EmulationPolicy)` in `crates/app/src/plugin_host/bundles.rs` stages the archive with the new platform `stage_bundle_archive`, runs each component through `stage_package_archive` and the shared `inspect_staged_package` entry point (the body `ManagedPackageLifecycle::inspect_candidate` also uses), and returns `BundleCandidateReport` with the bundle identity plus one `BundleComponentInspection` per component (plugin id, kind, version, archive path, availability, and the existing `PackageCompatibilityReview`). A component archive/manifest failure returns `BundleInspectionError::Component` for the whole bundle; only a `selected_target` failure becomes `BundleComponentAvailability::Unavailable` with a typed reason, leaving siblings available. Fixtures are real `scripts/plugin-package.py keygen/pack/bundle` output: a two-component bundle reports both components available, a Windows-only extension is unavailable on the Linux host while its provider sibling stays available, a modified component archive fails bundle verification, and a component with an invalid API range (accepted by the pack tool but rejected by the host manifest validator) fails structurally. Verified with `CARGO_TARGET_DIR=/data/DADOS_LINUX/geral/area_troca_rust/target-bundle-inspection CARGO_NET_OFFLINE=true cargo test --locked -p gamenode-app --lib plugin_host::bundles` (4 passed), `... -p gamenode-platform --lib plugin_bundles` (11 passed), `... -p gamenode-app --lib plugin_host::managed_packages` (3 passed), `cargo clippy --locked -p gamenode-platform --all-targets -- -D warnings`, `cargo clippy --locked -p gamenode-app --lib --tests -- -D warnings`, `cargo fmt --all -- --check` and `git diff --check`.

## 3. Registry and atomic install

- [x] 3.1 Add the `plugin_bundles` and `plugin_bundle_components` migration after the current latest version; verify a fresh database and an upgrade preserve existing packages and rows and that the schema hash fixture is updated. Added migration `0025_plugin_bundles` (schema 25), `SqlitePluginBundleStore` (`record_bundle`, `bundle`, `bundle_for_plugin`, `remove_bundle`) on the shared connection, the `docs/local-state.md` table entries, a schema-24 upgrade test and store tests; verified with `cargo test --locked -p gamenode-platform` (552 passed), `cargo clippy --locked -p gamenode-platform --all-targets -- -D warnings`, `cargo fmt --all -- --check` and `git diff --check`.
- [ ] 3.2 Implement staged bundle installation: write payloads, install each component through the existing registry path, write the bundle mapping in one transaction and clean staging on failure. Verify failure injection leaves no bundle mapping, no partial package and the previous installation usable.
- [ ] 3.3 Implement publisher-checked adoption of an already installed component and an explicit conflict when identity or publisher differs. Verify both cases with fixtures and confirm no silent overwrite.
- [ ] 3.4 Verify bundle removal deletes the mapping and component packages while preserving library history and grants policy; cover an active provider plus an unavailable extension.

## 4. Import review and consent

- [ ] 4.1 Build the aggregated import review DTO (bundle identity, publisher fingerprint, trust, license aggregate and embedded per-component reviews) and its Tauri command. Verify it is produced without executing package code and that a single fingerprint confirmation pins the bundle publisher.
- [ ] 4.2 Submit per-component permission decisions from one grouped review through the existing grant APIs; verify approving the bundle grants nothing, denying one component leaves the others usable, and each component keeps its own revision.
- [ ] 4.3 Add the grouped review UI with en/pt-BR labels, permission reasons per component and compatibility details; verify with component tests and mocks that a denied component is clearly scoped and that the review remains usable while signed out (read-only inventory, sign-in prompt).

## 5. Inventory and uninstall

- [ ] 5.1 Add optional bundle fields to `PluginInfo` and aggregate components by bundle in the host inventory. Verify an installed bundle returns one bundle entry with both component statuses and unchanged standalone packages.
- [ ] 5.2 Render one Stores entry per bundle with bundle name, version, publisher and per-component status; keep enable/disable per component in the details. Verify keyboard behavior, unavailable extension state and official-plugin isolation in component tests.
- [ ] 5.3 Implement one bundle uninstall action: stop every component with the bounded grace, remove packages and mapping, and apply one persistence-retention choice. Verify no process survives, data retention behaves as chosen and a partial failure reports rather than silently claiming success.

## 6. Update and rollback

- [ ] 6.1 Extend the update index and review to carry a bundle hash and per-component old/new versions and permission diffs. Verify an index without the bundle field still updates standalone packages and a mixed-bundle update is rejected.
- [ ] 6.2 Implement atomic bundle activation and rollback: stage every component, activate all or none, and revalidate grants without widening. Verify a failed staging/activation keeps the previous set active and rollback restores it with the same grants.
- [ ] 6.3 Add bundle update/rollback UI states and localized errors. Verify a slow update leaves unrelated UI usable and a persistence error restores the displayed controls to the saved state.

## 7. Integration and acceptance

- [ ] 7.1 Package the Dolphin plugin (`richmont/gamenode-plugin-dolphin-emu`) provider and launch packages into one bundle; verify import, one review, one inventory row and one uninstall on Linux with the production validators.
- [ ] 7.2 Record the Linux acceptance evidence: provider lists games, extension status is correct, uninstall removes both and data retention follows the chosen option. Cross-build the Windows bundle and record native Windows acceptance separately.
- [ ] 7.3 Update `docs/plugin-development.md`, the plugin example READMEs and the Dolphin repository `README`/`TASKS` with the bundle flow. Verify the documented commands and links resolve and OpenSpec strict validation passes.

## Workflow follow-up

- Archive this change after the review and native acceptance requirements are satisfied.
- Synchronize the modified main specs (`desktop-external-plugins`, `desktop-settings`) when archiving.
- Add the bundle artifact to the release notes and the plugin publication guide.
