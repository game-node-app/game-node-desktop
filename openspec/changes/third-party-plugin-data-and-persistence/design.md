# Design

## Boundary and dependencies

This follow-up depends on the MVP's authenticated plugin identity, profile fencing, permission revisions and package lifecycle. It does not enable cross-plugin visibility in the MVP. It may be implemented independently of custom launch after those foundations are complete.

## Shared data

Keep plugin-private storage separate from published datasets. Attribute every record to a host-derived owner and package provenance. Queries and subscriptions use explicit per-query selectors and default-deny category/record grants. Apply projection, size/depth limits, cursor fencing and revocation to every delivery path.

## Persistence and administration

Durable records use a logical owner namespace and explicit retention choice. Quotas, cleanup and uninstall retention operate only on the authenticated owner's data. Host administration may report physical usage, but plugin IPC never receives physical paths. Diagnostics and gameplay observations remain plugin-produced data; Core does not add collectors or dashboards here.
