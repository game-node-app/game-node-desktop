# Design

## Boundary and dependencies

This follow-up depends on the MVP's authenticated plugin identity, profile fencing, permission revisions and package lifecycle. It does not enable cross-plugin visibility in the MVP. It may be implemented independently of custom launch after those foundations are complete.

## Shared data

Keep plugin-private storage separate from published datasets. Attribute every record to a host-derived owner and package provenance. Queries and subscriptions use explicit per-query selectors and default-deny category/record grants. Apply projection, size/depth limits, cursor fencing and revocation to every delivery path.

## Persistence and administration

Durable records use a logical owner namespace and explicit retention choice. Quotas, cleanup and uninstall retention operate only on the authenticated owner's data. Host administration may report physical usage, but plugin IPC never receives physical paths. Diagnostics and gameplay observations remain plugin-produced data; Core does not add collectors or dashboards here.

## Publisher key change and data migration

A changed publisher fingerprint creates a distinct security identity even after explicit re-trust. Keep old data isolated and inert until the user reviews and explicitly requests migration of compatible data. The host previews source, destination and eligible data, checks schema compatibility and reports conflicts or partial failures; preserve old data and provenance until separately deleted. Never migrate secrets or grants automatically; access and retention require their own current approvals. This is separate from ordinary version upgrades under the same pinned identity.
