# Plugin endpoint acceptance with real consumers

Every plugin endpoint in the public contract (`PluginHost` and the provider exchange stream) is
homologated only when a **real consumer** crosses it: a signed bundle running under the host, with
the user's grants, lifecycle and process supervision. Rust fixtures that construct the happy state
directly are regression tests, not homologation.

The Dolphin integration proved why. The bugs it found were not Dolphin-specific; all of them lived
in shared host paths that no real third-party bundle had crossed before:

| Fix | Shared path |
| --- | --- |
| `52eeabc` target revision on custom-launch checks | `GamesProposeCustomLaunch` / `GamesRequestCustomLaunch` |
| `356a759` runtime recipe with no detected target | recipe validation (`base_target_revision = 0`) |
| `ef072cd` launch selection without `target_json` | saved recipe selection |
| `02d665c` desktop session environment for runtimes | launch supervisor |
| `0c42f00` remove an uninstalled provider by plugin id | live lifecycle |
| `2946c3e` swap a provider process config on accepted update | live lifecycle |
| `481289e` offer extension launch when a store has no native play | launch UI |

The recurring classes are: revision/epoch bookkeeping, absent or zero state (no target, no catalog,
no account), process environment inheritance, lifecycle changes without a restart, store-shaped UI
assumptions, and opaque ids leaking into labels.

## Status legend

| Status | Meaning |
| --- | --- |
| **third-party** | A signed third-party bundle crossed it on a real host. |
| **official** | Only a bundled official provider (Steam, Epic, PackageKit) crossed it. It proves the host path but not third-party parity. |
| **example** | Only a reference example covers it, usually in test fixtures. |
| **fixture** | Only Rust fixtures cover it. No process, no user consent. |
| **none** | Advertised with no consumer evidence. |

A row is **homologated** only at **third-party** status and after the full sequence: grant → use →
revoke in flight → profile change → uninstall/reinstall.

## Coverage snapshot — 2026-11-22

85 routes advertised, 2 deferred (`PluginHost.OpenSession` and `PluginHost.SubmitDiscovery` are
intentionally unadvertised; discovery runs through the exchange stream). Roughly **27 of 85**
advertised routes have been crossed by a real third-party bundle, and most of those only on the
grant path.

| Family | Routes | Status | Consumer / evidence |
| --- | --- | --- | --- |
| Control plane | `OpenSession`, `Exchange`, `GetContext`, `DescribeApi`, `GetPermissions`, `WatchPermissionChanges` | third-party | Dolphin provider and launch |
| Discovery | `Discover` (exchange stream) | third-party | Dolphin provider |
| Catalog | `Catalog` (exchange stream) | official | Steam, Epic |
| Account | `Account` (exchange stream) | official | Steam, Epic |
| Login, cookies, page | `Login` (exchange stream) | official | Steam, Epic |
| Secrets | `Secret` (exchange stream) | official | Steam, Epic |
| Playtime counter | `Counter` (exchange stream) | official | Steam |
| Declared actions | `DeclareActions` (exchange stream) | official | Steam, Epic |
| Achievements | `ReadAchievements` (exchange stream) | official | Steam on Linux; native Windows acceptance open (`add-achievement-tracking` 8.2/8.3) |
| Library read | `LibraryList`, `LibraryGet`, `LibraryWatch` | third-party | Dolphin launch (`LibraryGet` on the selected entry) |
| Metadata write | `MetadataUpdate` | none | — |
| Favorites | `FavoritesList`, `FavoritesSet` | none | — |
| Matching | `MatchingSearch`, `MatchingAssign` | none | — |
| Artwork | `ArtworkRead`, `ArtworkSet` | none | — |
| Standard launch | `GamesRequestLaunch` | none | — |
| Install | `GamesRequestInstall` | none | — |
| Custom launch | `GamesProposeCustomLaunch`, `GamesRequestCustomLaunch`, `GamesRemoveCustomLaunch` | third-party | Dolphin launch |
| Runtimes | `RuntimesList`, `RuntimesGet`, `RuntimesWatch`, `RuntimesRequestAccess`, `RuntimesRegister`, `RuntimesUpdate`, `RuntimesRemove` | third-party (partial) | Dolphin used read and request-access; update/remove only in fixtures |
| Runtime content | `GamesRegisterContent` | none | Dolphin carries a content handle in the recipe without calling register |
| Logical libraries | `LibrariesList`, `LibrariesGet`, `LibrariesWatch`, `LibrariesCreate`, `LibrariesUpdate`, `LibrariesRemove`, `LibrariesAddRoot`, `LibrariesUpdateRoot`, `LibrariesRemoveRoot` | none | — |
| Files read | `FilesRequestAccess`, `FilesList`, `FilesRead` | third-party (partial) | Dolphin requested a folder and read a ROM; `FilesList` unproven |
| Files write/delete | `FilesWrite`, `FilesDelete` | none | — |
| Network | `NetworkFetch` | none | — |
| Private storage | `StorageGet`, `StoragePut`, `StorageDelete` | example | `rust-configuration` for get/put; delete unproven |
| Persistence | `PersistenceGetUsage`, `PersistenceListEntries`, `PersistenceDeleteEntries`, `PersistenceClear`, `PersistenceSetRetentionPolicy` | none | — |
| Data publish | `DataPublish` | third-party | Dolphin catalog dataset (persisted) |
| Data read | `DataGet`, `DataList`, `DataWatch` | none | — |
| Common categories | `common.*` selectors | none | — |
| Notifications | `NotificationsPublish` | none | — |
| Jobs | `JobsGet`, `JobsWatch`, `JobsCancel` | third-party (partial) | Dolphin launch job; cancel unproven |
| Backups | `BackupsList`, `BackupsRequestCreate`, `BackupsRequestRestore` | none | — |
| Save profiles | `SavesListProfiles`, `SavesGetProfile`, `SavesCreateProfile`, `SavesUpdateProfile`, `SavesRemoveProfile` | none | — |
| Approved-place inspection | `LocationsInspect`, `LocationsCompare`, `LocationsGetAccess`, `LocationsGetFingerprint`, `LocationsGetLastModified`, `LocationsGetSize`, `LocationsWatchAvailability` | none | — |
| Tools | `ToolsList`, `ToolsRequestApply`, `ToolsRequestRemove` | none | — |
| PC ports | `PortsList`, `PortsDetail`, `PortsRequestInstall` | none | Linux install is `UNAVAILABLE` by design |
| Storage diagnostics | `StorageDiagnosticsPublishBenchmark`, `StorageDiagnosticsPublishSmart` | none | — |
| Hardware | `ComputerPublishHardware` | none | — |
| Presence | `GamePresencePublish`, `GamePresenceClear` | none | — |
| Performance | `GamePerformancePublishSamples`, `GamePerformanceEndSeries` | none | — |
| Sessions | `game.sessions.report` | none | — |
| Declarative form | `ui.settings` | none | — |
| Configuration window | `ui.configure` | example | `rust-configuration` |

## Cold paths, in risk order

These rows carry the most consent surface and therefore repeat the Dolphin failure pattern first.

1. **Files write/delete and widening a read approval to write.** The dialog must show what was
   already approved versus what is added; revocation must fence open handles.
2. **`NetworkFetch`.** Exact-origin approval, redirect chain, revocation during a fetch, widened
   scopes on package update.
3. **Backups and save profiles.** External-process detection, leases, exclusive launch/restore
   locks, per-game scope review.
4. **Tools and PC ports.** Install jobs, cancellation, idempotency, `UNAVAILABLE` on Linux.
5. **Data read side.** Recipient authorization, projections, retention, `DataWatch` cursors.
6. **Typed telemetry.** Large payload validation, ordering/revision, partial publication.
7. **`ui.settings`.** Closed-schema rendering and precedence over `ui.configure`.
8. **Matching, artwork, favorites and metadata writes.** Revision conflicts, de-duplication,
   platform filter.
9. **Logical libraries and roots.** Root approval, watch and revocation.

## Homologation procedure

For each row:

1. Install the signed bundle and grant only the permission under test.
2. Run the consumer path and capture the observable result (report row, notification, launch plan).
3. Revoke the grant while the operation is in flight or while a handle is open.
4. Change the account profile and confirm the old context is fenced.
5. Uninstall, reinstall and confirm the row still works and no host-owned association is lost.
6. Record the status and evidence on this page.

## Maintenance

This page is the source of truth for endpoint homologation. When a route is added or an endpoint
changes, its row must be added or updated in the same change. A row at `fixture` or `none` is an
open acceptance gap, not a completed feature. `scripts/check-plugin-conformance.py` already
verifies that every advertised route exists; it does not yet verify that every route has a
consumer, which is the job of the probe bundle proposed in
`openspec/changes/add-plugin-endpoint-probe/`.
