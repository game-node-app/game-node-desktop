# Design

## Boundary and dependencies

This follow-up depends on the MVP's authenticated plugin identity, profile fencing, permission revisions and package lifecycle. It does not enable cross-plugin visibility in the MVP. It may be implemented independently of custom launch after those foundations are complete.

## Shared data

Keep plugin-private storage separate from published datasets. Attribute every record to a host-derived owner and package provenance. Queries and subscriptions use explicit per-query selectors and default-deny category/record grants. Apply projection, size/depth limits, cursor fencing and revocation to every delivery path.

`DataListReply.revision` carries the global broker revision of the returned snapshot. A client passes that exact revision to `DataWatchRequest.after_revision`; the host accepts only a current resumable position and fails closed for stale positions, so the list-to-watch handoff cannot skip intervening records.

## Persistence and administration

Durable records use a logical owner namespace and explicit retention choice. Quotas, cleanup and uninstall retention operate only on the authenticated owner's data. Host administration may report physical usage, but plugin IPC never receives physical paths. Diagnostics and gameplay observations remain plugin-produced data; Core does not add collectors or dashboards here.

## Updates, downgrades and private data

An update feed cannot select a release below the active version. A user-requested downgrade uses the same signature, integrity, target and API verification path as an update, shows the active and target versions with a downgrade warning, and changes the active package only after explicit confirmation. A publisher-key change requires a separate re-trust review bound to the exact new fingerprint; approving a package update does not substitute for that review.

The private-data owner namespace remains stable across package versions and continues to use the authenticated plugin, publisher and profile identity from the namespace resolver. A plugin that changes a private-data schema writes to versioned logical keys or schema identifiers. It does not automatically migrate destructively or delete the older values; retaining them allows a compatible last-known-good version to recover its data. Package rollback keeps the current grants intersected with the selected version's manifest. Reuse of data archived by uninstall remains a separate host-reviewed operation and does not grant a new consumer access.
