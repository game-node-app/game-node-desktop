# Design

## Boundary and dependency

This is the second delivery slice after `add-third-party-plugins` (provider/catalog MVP). It depends on the MVP's authenticated account-bound session, permission revisions, async Tauri command bridge, plugin inventory and bounded process lifecycle. It does not change official provider routes.

## Game actions

The host issues a short-lived, single-use intent only from a user action. Launch/install requests without a valid intent remain pending for explicit confirmation and never fall back to direct execution. The intent binds plugin, publisher, account epoch, selected game/installation, operation and, for launch only, detected-target revision. Confirmation is cancel-first, time-bounded and can remember approval only for the exact scoped operation and, for launch only, target revision. Revocation and job cancellation fence pending work.

## Filesystem approvals

Folder approvals are host-owned and separate from API permission grants. Show the requesting plugin, publisher, game, folder purpose, operations, affected games and plugin-suggested folders as unverified. Persist only the selected, canonicalized scope. Revocation closes affected handles and fences in-flight work. Use asynchronous commands and the shared `AppModal`; never block UI callbacks.

## Configuration lifecycle

A Configure action opens or focuses one plugin-owned window per plugin/profile. The host keeps window handles opaque, correlates lifecycle results, and fences them on account/profile changes, disable, revocation, update or removal. Declarative settings remain a separate optional, closed-schema fallback; no plugin code is rendered inside the GameNode frontend.

## Installation confirmation and network origins

A user intent starts the host install workflow but never substitutes for its preview, validation and explicit confirmation. Bind the detected-target revision only to launch intents. Persist exact HTTPS origins under the authenticated plugin, pinned publisher and account profile; widening requires review and revocation or profile changes fence in-flight fetches. This connects MVP 10.2/10.4 permission review to the existing 6.3 network port.
