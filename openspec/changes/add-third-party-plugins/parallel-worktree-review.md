# Review of the four plugin worktrees

2026-11-18. Counts use the integrated OpenSpec lists. Evidence comes from
committed code and existing validation records. This review ran no new tests.

| Front | Reviewed commit | Complete | Open |
| --- | --- | ---: | ---: |
| Provider/catalog MVP | `4f8dab0` | 63/77 | 14 |
| Actions and configuration | `2dd7d54` | 7/10 | 3 |
| Custom launch and runtimes | `e45973d` | 8/20 | 12 |
| Shared data and persistence | `6330a94` | 6/28 | 22 |
| Total | | 84/135 | 51 |

All three child commits are already ancestors of the integration branch.
The child lists predate two actions tasks (1.9–1.10), three runtime tasks
(1.18–1.20) and one data task (1.28). Use the integrated lists when assigning
work. Data 1.27's production wiring is also ahead of its child branch.
Bring the required integrated baseline into each child before starting its
assigned task, preserving checkout-local edits.

## Remaining work by front

- **MVP.** 10.4 closed after the offline reader passed. 10.5, 10.8, 11.1,
  11.3 and 11.9 have code or tooling in place and still have gaps noted in
  the checklist. 11.2 needs the standalone Rust provider against the public
  SDK. 11.5–11.7 need account fixtures, example packaging and conformance.
  18.1–18.4 and 18.10 are acceptance tasks. Existing tests cover parts of
  those flows. Linux and Windows native acceptance and the full isolation
  and official-provider matrices are still outstanding.
- **Actions.** 1.8 needs the production approved-origin source and UI consent
  backed by the permission store. 1.9 needs the install confirmation flow
  verified through both intent paths. 1.10 needs full fixture evidence for
  both configuration examples, especially Rust.
- **Runtimes.** 1.9, 1.15 and 1.16 have tracker, declarations and preference
  code. 1.18 has scoped supervised startup from actions 1.6. 1.20 has Go
  wire fixtures. The remaining criteria for each are in the checklist.
  1.10 needs two-client runtime/library projections, 1.11 and 1.17 need
  scoped discovery and its acceptance, 1.12–1.14 need launch and native
  environment acceptance, and 1.19 needs same-process transitions and
  freshness verified through the authenticated context.
- **Data.** 1.5, 1.6, 1.13 and 1.27 have partial implementation. 1.7–1.8
  need scoped metadata patching and shared-resource backup coordination.
  1.9–1.12 need typed diagnostics, hardware, presence and performance
  flows and their fixtures. 1.14–1.17 and 1.19 need durable-store
  administration, retention, archive and cleanup. 1.20–1.23 need examples
  and combined API acceptance. 1.24–1.25 need publication and operation
  consent. 1.28 needs reviewed migration across publisher identities.

## Suggested execution order

| Front | Start in parallel | Next dependencies |
| --- | --- | --- |
| MVP | **11.1**. Verify the source archives in a clean directory. | 11.2 follows 11.1. 11.3 can finish independently on native transport hosts. Both examples feed 11.5–11.7 and 18.1. Pair 10.8 with 11.9, then run native acceptance 18.2–18.3. Prepare 18.4 and 18.10 fixtures alongside this work. |
| Actions | **1.8**. Wire exact origins into the persisted account-bound permission store. | 1.9 follows with the install confirmation flow. Treat 1.10 as a fixture task and coordinate its Rust example changes with MVP 11.2. |
| Runtimes | **1.18**. Start with the missing process lifecycle fixtures and isolate any resulting host fixes. | 1.18 unblocks 1.19. Finish 1.9 with 1.15–1.16 once transitions and accounting are verified. 1.11 feeds 1.17. Coordinate 1.10 with data projections, 1.20 with 1.13, and 1.14 with MVP 10.8. |
| Data | **1.27**. Start with the two-plugin public RPC fixture using persisted host policy and trace separate retention consent. | 1.27 unblocks 1.5 acceptance. Coordinate any recipient retention implementation with 1.13. Close 1.6 before 1.7 and the metadata examples in 1.21. Data 1.8 can advance using actions 1.5. The diagnostics/presence/performance tasks feed 1.12. Durable storage and cleanup feed 1.19 and 1.22. Runtime identity and the public examples feed 1.20. |

The first four tasks can start together in separate worktrees. Keep the
initial changes in release tooling, network approval, process fixtures and
Data RPC fixtures respectively. Coordinate before editing shared
`commands.rs`, `PluginInventory.tsx`, `plugin.proto` or the common grant/profile
code. The three child branches still document the Pi target. Before validating
on this workstation, bring in the workstation target instructions, verify the
effective path with Cargo metadata and run Cargo validation sequentially. Share
fixture results so acceptance evidence is recorded once.
