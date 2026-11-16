# Design

## Boundary and dependency

This is the second delivery slice after `add-third-party-plugins` (provider/catalog MVP). It depends on the MVP's authenticated account-bound session, permission revisions, async Tauri command bridge, plugin inventory and bounded process lifecycle. It does not change official provider routes.

## Game actions

Core exposes an explicit **Start with extension** action on a game. The user chooses an installed, trusted, enabled extension and either Play or Install. Core resolves the selected installation and target itself, checks the current profile and extension grants, then starts that extension in a one-shot supervised `Extension` session. The extension cannot provide or widen its target context.

For that action only, the host issues a short-lived, single-use intent bound to plugin, publisher, account epoch, selected game/installation, operation and detected-target revision. It passes the opaque intent id to the extension as `GAMENODE_PLUGIN_USER_INTENT_ID` at process startup; the Rust SDK exposes it on `Launch`, while other SDKs can read the documented environment variable. Provider sessions and extension sessions not started by this Core action receive no intent. The extension must submit the same game action and binding to spend the intent. Any missing, expired, reused, foreign or mismatched intent follows the ordinary explicit-confirmation path; it never authorizes direct execution by itself.

Launch/install requests without a valid intent remain pending for explicit confirmation and never fall back to direct execution. Confirmation is cancel-first, time-bounded and can remember approval only for the exact scoped operation and target revision. Revocation, profile changes, process exit and job cancellation fence pending work and terminate the one-shot extension process.

## Filesystem approvals

Folder approvals are host-owned and separate from API permission grants. Show the requesting plugin, publisher, game, folder purpose, operations, affected games and plugin-suggested folders as unverified. Persist only the selected, canonicalized scope. Revocation closes affected handles and fences in-flight work. Use asynchronous commands and the shared `AppModal`; never block UI callbacks.

## Configuration lifecycle

A Configure action opens or focuses one plugin-owned window per plugin/profile. The host keeps window handles opaque, correlates lifecycle results, and fences them on account/profile changes, disable, revocation, update or removal. Declarative settings remain a separate optional, closed-schema fallback; no plugin code is rendered inside the GameNode frontend.
