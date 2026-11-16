# Design

## Boundary and dependencies

This follow-up depends on the MVP's authenticated plugin identity, profile fencing, permission revisions and package lifecycle. The generic verified-package import, activation lifecycle, manifest permission replacement and requested/granted/effective permission inventory remain owned by `add-third-party-plugins` tasks 10.2 and 10.4. This change consumes those foundations and owns the shared-data-specific policy UI, persistence and enforcement; it does not duplicate package lifecycle or generic permission administration.

## Shared data

Keep plugin-private storage separate from published datasets. Attribute every record to a host-derived owner and package provenance. Queries and subscriptions use explicit per-query selectors and default-deny category/record grants. Apply projection, size/depth limits, cursor fencing and revocation to every delivery path.

Connect the activation review's common-category and selected-record choices to the persisted `SharedSharingPolicies` store. Resolve the policy key from the host-authenticated plugin, publisher fingerprint and active profile; never accept those identity parts from plugin RPC data or an unverified UI value. Start with empty grants. Persisting a grant only makes matching records eligible: each query still needs its explicit selector, and saving the grant must not run a query or deliver records.

Keep sensitive/custom sharing separate from common category grants. The host review must identify the exact source, recipient and dataset, and the allowed fields and history/live delivery modes. Persist the decision through `SharedSharingPolicies`; a common-category grant cannot widen it, and plugin RPC cannot create or expand it. Revocation updates the policy revision and fences queries, cursors, queued events and subscriptions. Recipient retention remains a separate consent governed by the Persistence work; a read or sharing policy never grants durable storage.

The data-specific review controls build on the generic plugin activation and permission inventory work in `add-third-party-plugins` 10.2/10.4. Policy writes from Tauri must be asynchronous and move filesystem persistence off the UI thread. Task 1.5's producer/consumer contract is verified only after this integration provides a user-authorized public flow.

`DataListReply.revision` carries the global broker revision of the returned snapshot. A client passes that exact revision to `DataWatchRequest.after_revision`; the host accepts only a current resumable position and fails closed for stale positions, so the list-to-watch handoff cannot skip intervening records.

## Persistence and administration

Durable records use a logical owner namespace and explicit retention choice. Quotas, cleanup and uninstall retention operate only on the authenticated owner's data. Host administration may report physical usage, but plugin IPC never receives physical paths. Diagnostics and gameplay observations remain plugin-produced data; Core does not add collectors or dashboards here.

## Updates, downgrades and private data

An update feed cannot select a release below the active version. A user-requested downgrade uses the same signature, integrity, target and API verification path as an update, shows the active and target versions with a downgrade warning, and changes the active package only after explicit confirmation. A publisher-key change requires a separate re-trust review bound to the exact new fingerprint; approving a package update does not substitute for that review.

The private-data owner namespace remains stable across package versions and continues to use the authenticated plugin, publisher and profile identity from the namespace resolver. A plugin that changes a private-data schema writes to versioned logical keys or schema identifiers. It does not automatically migrate destructively or delete the older values; retaining them allows a compatible last-known-good version to recover its data. Package rollback keeps the current grants intersected with the selected version's manifest. Reuse of data archived by uninstall remains a separate host-reviewed operation and does not grant a new consumer access.
