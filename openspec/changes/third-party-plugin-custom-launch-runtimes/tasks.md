# Tasks

This follow-up is part of the delivery split documented in `../add-third-party-plugins/delivery-split.md`. Source task IDs are retained for traceability.

- [x] 1.1 **Source 7.8.** Implement game.custom_launch recipe propose/request/remove with alternate executable/URI, argument vectors, working directory, approved external runtime handles and environment set/unset; verify actual worker-spawned fixture processes receive every field and incapable adapters/URI handlers fail explicitly rather than ignoring environment.

- [x] 1.2 **Source 7.9.** Implement per-game/profile plugin-owned overlays for official, third-party and manual games, with independent normal/custom grants and one-use host user intents; verify an extension can target a selected Steam game without provider impersonation, broad library reads or modification of official state.

- [x] 1.3 **Source 7.10.** Fence approvals to recipe/base-target/resource revisions, sanitize child environment and associate wrapper/child process attribution with the original game; verify revocation and changed recipes block new launches without killing running games, leaking session credentials or silently switching routes.


- [x] 1.4 **Source 7.11.** Build immutable typed LaunchPlan resolution and versioned canonical approval fingerprints, binding source/package/component identities, effective target/argv/cwd/env and resource/grant revisions; verify plan substitution, replay, expiry and changed known mod/runtime components cannot dispatch through an old approval, with hashing/revalidation off the UI thread.


- [x] 1.6 **Source 10.7.** Implement the shared Launch plan / Plano de lançamento review with source provenance, ordered known components, complete literal arguments/paths/environment delta and differences; verify Cancel default, Run once, exact-recipe remembered approval, explicit secret reveal, accessibility, denied-request no-bypass behavior and scope-not-safety wording in en/pt-BR.

- [x] 1.5 **Source 10.6 (depends on 1.6).** Add per-game launch recipe selection and explicit return to detected default using the shared host launch-plan review for runtime/emulator, mod-loader/layer and custom target/parameter changes; verify unchanged approved Play needs no repeated prompt and changed/revoked plans require an actionable choice in both locales.


- [x] 1.7 **Source 11.8.** Add equivalent safe Go/Rust launch-consent fixtures for emulators/runtimes, known mod layers and environment-only customization; verify both clients use host review and cannot replace approved fields or treat launch consent as installation/write permission.


- [x] 1.8 **Source 14.2.** Add approved runtime/interpreter resources and per-content game/installation bindings to structured launch recipes; verify several games share a runtime and one registered ROM/GameNode game offers several runtimes without duplicate game identity, lost history or save changes. Cover moving/renaming content, distinct variants, binding migration and runtime removal without cascading file/game/save deletion.

- [ ] 1.9 **Source 14.3.** Define and implement the common typed runtime-session event/result family over Exchange under default-deny per-game/runtime game.sessions.report, with plugin-owned adapters and Core arbitration; verify same-PID content switches, concurrent instances, duplicate evidence, PID reuse, boot failure, stale/disconnected/revoked reports, unknown gaps and fresh confirmation after restart. Preserve direct-executable, account, source, checkpoint and platform policies, and apply manifest-declared accounting modes, exclude confirmed pauses and honor the user disable control. Document that one plugin may support multiple runtimes and that descriptive presence cannot establish sessions; test consented Data projections without exposing handles or credentials.
  - Review (2026-11-18): The integrated `e45973d` adds typed Exchange reports, the tracker and fixtures. Keep open for 1.18 and 1.19, acceptance of accounting behavior and consented Data projections.


- [ ] 1.10 **Source 14.4.** Exercise A registering an owned runtime with a custom compatibility extension and B querying it through Runtimes.List/Get/Watch under the applicable category read permission; verify the private-only default, explicit common-data opt-in, same-console ROM/runtime availability through Library.List/Get/Watch without file paths or bytes, no pairwise grant and no mutation/execution authority.


- [ ] 1.11 **Source 14.5.** Add negotiated scoped runtime-content discovery through Games.RegisterContent for runtime plugins/extensions under library.discovery without provider impersonation; verify ZSNES/bsnes observations reconcile one registered game/content with selectable bindings, discovery grants no execution, and DeSmuME-to-melonDS binding migration preserves history and save references without requiring foreign deletion.


- [ ] 1.12 **Source 18.5.** Exercise detected and custom launches across official, third-party and manual game fixtures, including a Linux Proton-style runtime wrapper and direct executable bypass; record equivalent Go/Rust parameters, original game identity, explicit unsupported-target outcomes and unchanged official provider files/credentials.


- [ ] 1.13 **Source 18.8.** Exercise common launch-plan consent end to end for runtime, mod and parameter-only overlays on official/third-party game fixtures; record accurate provenance/differences, changed-plan refusal, unchanged remembered Play, no unauthorized operation bypass and explicit limits on native-program behavior certification.


- [ ] 1.14 **Source 18.9.** Record native Windows/Linux environment-detection acceptance separately from pure evaluator fixtures, including execution userspace versus host, pre-launch revalidation and compatibility failure UI; do not treat a successful declaration check as native code safety certification.


- [ ] 1.15 **Source 19.1.** Add manifest playtime/pause capability declarations and developer-selected disabled/application-open accounting; verify signed validation, update disclosure, excluded confirmed pauses, unidentified runtime content and preservation of official accounting policies.
  - Review (2026-11-18): Signed manifest declarations, host reporting checks and update disclosure are implemented in `e45973d`. Verify pause exclusion and unidentified-runtime and official accounting behavior together with 1.9.

- [ ] 1.16 **Source 19.2.** Add installation/settings disclosure and a persistent per-plugin accounting disable control, including the specified pt-BR warning; verify preserved history, independent discovery/launch and deduplicated evidence from other sources.
  - Review (2026-11-18): Profile persistence and inventory controls are implemented in `e45973d`. Verify the warning, preserved history, operation independence and deduplication across sources.

- [ ] 1.17 **Source 19.5.** Verify provider and host-issued scoped runtime-content discovery contexts end to end, preserving legacy provider validation while accepting authorized provider-free extensions and rejecting missing/foreign contexts.

- [x] 1.18 **New: provider-free extension process lifecycle.** Extend package discovery and the supervised process manager to support `extension` packages without registering a provider id or provider-owned services. Persist enablement per signed-in account; require a user-selected target-game context and current extension identity, grants and profile epoch before session start; fence and stop the process tree on disable, removal, sign-out, account change, revocation, crash and update. Reuse package signature, compatibility, bounded readiness and rollback checks while negotiating an extension session, and expose explicit runtime availability to inventory. Verify extension sessions cannot call provider-only RPCs or name another game, never inherit a provider identity, and cannot activate while runtime support or target context is unavailable.
  - Review (2026-11-18): Production registration and scoped one-shot extension launch already exist (actions 1.6). Keep open for the full readiness, rollback and stop/fencing matrix for provider-free extensions.
  - Evidence: Linux signed process fixtures verified profile-scoped enablement, foreign target and provider RPC denial, runtime inventory availability, session fencing and descendant termination. `OpenSession` without `Ready` restored the old version and configuration. A valid `Ready` committed activation, and the public rollback flow negotiated readiness again without reusing the game action. The focused app test run passed 26 tests.
  - Follow-up evidence: The Plugins screen requires an explicit eligible game for idle extension update and rollback. Cancel leaves the registry and process slot untouched. Linux regressions verified a readiness-only Extension session without a user-intent token, launch and install denial even after Ready, teardown before the version switch, and rejection of stale target, profile and grant revisions. The focused Rust run passed 14 tests and the Plugins screen suite passed 34 tests, including update in English and rollback in Brazilian Portuguese.

  - Review follow-up (2026-11-18): an independent review found that `Engine::dispatch` now waits on the profile transition lock, which extension update activation holds across a durable write, while several async paths (sign-in, session check, sign-out finish, 401 handling, playtime rejection) still called it directly on tokio workers. Those paths now use `dispatch_on_worker`, `check_rejected` is async, and the update/rollback approval handlers resolve the readiness target on a blocking worker. The review found no identity, race, fencing or compatibility defects in the extension lifecycle. After merging with the network-origin and compatibility work on the integration branch: 779 app library tests pass and 2 fail (`every_missing_host_grant_is_rejected_before_adapter_or_domain_effects` and `custom_launch_without_its_own_grant_is_denied_and_describe_lists_the_routes`), both of which also fail at the pre-merge integration base and are independent of this change; the other five extension-session `Denied` failures at that base no longer occur. Workspace tests outside the app crate, `cargo check --workspace --all-targets`, `tsc` and 956 UI tests pass; one palette integration test is timing-sensitive under heavy load and passes in isolation.
- [ ] 1.19 **Same-process game transitions and freshness.** Complete 1.9 using instance identity independent of game/installation; validate each new context and grant before switching A to B on the same PID without restart. Verify distinct games and installations, denied transitions, PID reuse, unknown gaps, 30-second expiry and fresh confirmation after restart; document expected report interval and timestamp policy. This is a prerequisite for closing 1.9 and depends on 1.18.
- [ ] 1.20 **Cross-client launch review evidence.** Verify Go and Rust clients exercise host review and denial against a host fixture; coordinate with 1.7 and 1.13 so evidence is recorded once.
  - Review (2026-11-18): Go wire fixtures cover review/denial. Shared host fixture evidence across Go and Rust remains open.

Runtime 1.10 depends on data-and-persistence 1.27 for persisted common-data policy; common category reads do not require pairwise grants. Tasks 1.11/1.17 require 1.18 and the approved scoped discovery context. Runtime save-resource coordination depends on actions folder approvals and data task 1.8.
