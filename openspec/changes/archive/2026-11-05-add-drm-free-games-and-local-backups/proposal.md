# Proposal

## Why

DRM-free games need to feel like first-class library entries without making the user research titles, pick metadata before playing, or configure a separate backup program. Splitting roadmap step 4 lets us make adding, identifying, tracking and protecting these games reliable before adding Epic, GOG and Ubisoft integrations.

## What Changes

- Add a Windows **Add game** button: choose an executable, get a playable local entry immediately, and attempt GameNode matching automatically in the background. Keep the existing Change match and no-match choices.
- Infer titles from executable product metadata, meaningful executable names and installation context, enriched by Ludusavi manifest titles, aliases, installation folders and optional launch hints. Auto-match only a unique, strong identity; ambiguous results remain suggestions.
- Add opt-in watched folders, such as `F:\NonSteamLibrary`, with bounded background discovery and a review list. **The user confirmed that discoveries require review before being added.** Installers, tools and ambiguous executables never silently become library entries.
- Persist a built-in `manual` provider, shown as **DRM-free**, with stable installation identities, direct executable launch, editable launch/tracking settings, repair and removal. Matching is optional and no download/install action is offered.
- Extend the existing process tracker with explicit DRM-free executable rules and launcher-to-game handoff. Track external starts, ignore maintenance processes, and distinguish a real game exit from sign-out, preference changes, checkpoints and application shutdown.
- Bundle a pinned Ludusavi Windows CLI and a baseline primary manifest. Integrate it behind a background adapter with private GameNode configuration, cached manifest updates and independent save-profile matching.
- Enable local automatic backups by default for supported, unambiguous DRM-free save profiles; expose global/per-game controls, Backup now, version history and explicit restore with preview and a recovery backup. Save bytes stay local regardless of the GameNode sync setting.
- Keep missing/ambiguous save support visible without preventing play. Local saves and Windows registry save data are supported; automatic restore and cloud saves are deferred.

## Capabilities

### New Capabilities

- `desktop-drm-free-games`: Quick executable adds, persistent installation/launch settings, reviewed folder discovery and repair/removal.
- `desktop-save-backups`: Bundled backup engine, manifest/save-profile lifecycle, local backup scheduling, history and recoverable restore.

### Modified Capabilities

- `desktop-game-matching`: Conservative automatic identification from local evidence, independent of provider ids, while retaining account-specific user overrides.
- `desktop-providers`: A built-in manual scanner and restricted direct executable actions alongside existing declared URI actions.
- `desktop-library`: Add game entry points, executable launch progress, and persistent unavailable manual entries.
- `desktop-local-state`: Durable manual installations, discovery decisions, inferred identities, save profiles and backup jobs.
- `desktop-playtime`: Explicit process policies and physical-exit events for DRM-free games. This capability is introduced by the prerequisite `add-playtime-background`, which must be completed and its specs synced/archived before applying this change.

## Impact

Touches `core` providers/library/matching/tracker and new discovery/backup state machines; Windows adapters and SQLite migrations in `platform`; asynchronous engine wiring, Slint UI and English/Portuguese strings in `app`; and Windows packaging/licensing. Reuses existing GameNode search and external-id APIs; requires no server changes. Manual games use measured local sessions with source `manual`, platform 6; session submission remains roadmap step 6.

Implementation updates `docs/mvp-roadmap.md` to split step 4 into DRM-free/local backups first and Epic/GOG/Ubisoft next, and to document the bundled native helper and explicit restore writes. It does not add store scanners, store authentication, cloud sync, Linux adapters, whole-disk discovery, arbitrary script execution or a general user manifest system. Existing store libraries retain their behavior.
