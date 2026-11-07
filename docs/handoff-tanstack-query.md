# Hand-off: TanStack Query hardening

**Status:** ready to implement
**Date:** 2026-11-06
**Area:** `ui/` (React + TanStack Query) and `crates/app` (Tauri commands)
**Owner:** next agent, follow the tasks in order

This document turns the TanStack Query review into concrete, reviewable changes. It
covers six items. Tasks are ordered by priority; each one is independently shippable.

## Scope

In scope:

1. Desktop-safe `QueryClient` defaults (`networkMode`, focus/reconnect refetch).
2. Remove the unused `useMutation` layer.
3. Stop shipping the 4.8 MB license notices at startup and split the bootstrap payload.
4. Order the snapshot event bridge before the first snapshot fetch and keep events authoritative.
5. Make snapshot/stream failures visible in development.
6. (Optional) Centralize query keys/options and align test clients with production.

Out of scope:

- Any new dependency. **Do not add** `@tanstack/react-query-devtools`,
  `@tanstack/react-form`, `@tanstack/react-hotkeys`, a router, or query persistence.
- Changing the core architecture: one `['snapshot']` cache entry fed by
  `engine:snapshot` events stays. Do not switch to per-slice query keys or to
  `invalidateQueries` for push events.
- Rust engine/state-machine behavior, styling, and unrelated tests.

Ground rules from [`AGENTS.md`](../AGENTS.md) and [`CONTRIBUTING.md`](../CONTRIBUTING.md):

- Do not block the UI thread; keep Tauri commands `async` even when they do no I/O.
- Keep unrelated changes separate, preserve existing contributor work.
- Do not commit, push, or rewrite history unless the user explicitly asks. If asked,
  use Conventional Commits (suggested split at the end of this document).
- Prefer `pnpm --dir ui test --run`, `cargo test --locked -p gamenode-app`, and the
  Vite preview over cross-building to Windows.

## Evidence: current state

Line numbers were accurate when this document was written; re-locate by symbol if
they have drifted.

| Fact | Where |
| --- | --- |
| `QueryClient` defaults: `staleTime: Infinity`, `retry: false` only | `ui/src/AppProviders.tsx:66-75` |
| One cache entry `['snapshot']`, filled by `setQueryData` from `engine:snapshot` | `ui/src/api/useSnapshot.ts:28-50` |
| Snapshot query: `staleTime/gcTime: Infinity`, `placeholderData: DEFAULT_SNAPSHOT` | `ui/src/api/useSnapshot.ts:83-98` |
| `['bootstrap']` query for locale/version/notices | `ui/src/api/useSnapshot.ts:100-107` |
| Eight `useMutation` hooks defined, none imported anywhere | `ui/src/api/useSnapshot.ts:154-200` |
| Components dispatch commands fire-and-forget (`commands.libraryInput(...)`, etc.) | `ui/src/features/**` |
| `get_bootstrap_state` returns a full `Snapshot` **and** `include_str!` of the notices | `crates/app/src/commands.rs:8-14, 188-198` |
| Notices file size | `licenses/THIRD-PARTY-NOTICES.txt` = **4,833,811 bytes** |
| Snapshot events are coalesced/throttled and de-duplicated at 100 ms | `crates/app/src/commands.rs:49, 60-140` |
| License modal is mounted only while open | `ui/src/features/settings/SettingsModal.tsx:304-306` |

Current startup cost: two IPC calls (`get_snapshot`, `get_bootstrap_state`), and one
of them serializes ~4.8 MB of notice text that the shell does not use.

---

## Task 1 (P0) — Desktop-safe Query defaults

**File:** `ui/src/AppProviders.tsx`

Replace `createAppQueryClient` with:

```ts
export function createAppQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // The Rust engine pushes authoritative state; queries never go stale on their own.
        staleTime: Infinity,
        // Local IPC is not HTTP. navigator.onLine must not pause queries on an offline machine.
        networkMode: 'always',
        retry: false,
        // Window focus/reconnect are web concepts; Tauri fires them often and neither
        // implies the engine changed. Events are the refresh mechanism.
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      },
    },
  });
}
```

**Why:** with the default `networkMode: 'online'`, React Query consults
`navigator.onLine` for `invoke()`-backed queries. On an offline desktop,
`get_snapshot`/`get_app_info` can be paused (`fetchStatus: 'paused'`) and never run,
so `useSnapshot` falls back to the default (signed-out) snapshot even though Rust has
a usable offline library.

**Test:** no behavior change needed in tests. Run `pnpm --dir ui test --run`; existing
tests create their own clients and are unaffected.

---

## Task 2 (P0) — Remove the unused mutation layer

**File:** `ui/src/api/useSnapshot.ts`

The following exports have no consumers (verified with `rg` over `ui/src`, including
tests). Delete `useDispatchAuthMutation`, `useLibraryInputMutation`,
`useActivateEntryMutation`, `useLaunchEntryMutation`, `useLaunchSelectedMutation`,
`useRescanLibraryMutation`, `useEditLocalGameMutation`,
`useSelectToolChannelMutation` (currently lines ~154-200), and remove `useMutation`
from the `@tanstack/react-query` import.

Remove now-unused type imports from the same file: `AuthInput`, `LaunchKind`,
`LibraryInput`, `Registration`, `ToolChannel`. Keep `AuthView`, `BootstrapState`
(later renamed), `LibraryView`, `LocalView`, `PlaytimeSource`, `Settings`,
`Snapshot`, `ToolsView`.

**Why:** commands are fire-and-forget events into a state machine whose result arrives
through `engine:snapshot`; `useMutation` would add no pending/error state that the UI
consumes, and an `onSuccess: invalidateQueries(['snapshot'])` would cause a redundant
full-state `get_snapshot` per click. This also removes the mismatch with
`docs/tauri-v2-migration-plan.md:82` (a historical plan; do not edit it).

**Verification:**

```sh
rg -n "useDispatchAuthMutation|useLibraryInputMutation|useActivateEntryMutation|useLaunchEntryMutation|useLaunchSelectedMutation|useRescanLibraryMutation|useEditLocalGameMutation|useSelectToolChannelMutation" ui/src
# expect no output
rg -n "useMutation|mutate\(" ui/src
# expect no output
pnpm --dir ui test --run
```

If a future screen needs to disable a button while a command is in flight, add a
single, local `useMutation` at that call site then — not a global facade.

---

## Task 3 (P0) — Split app info from the 4.8 MB license notices

Keep `get_snapshot`; replace `get_bootstrap_state` with two commands. This removes
both the unused `snapshot` field and the notices text from the startup path.

### 3a. Rust command contract

**File:** `crates/app/src/commands.rs`

Replace the `BootstrapState` struct (lines ~8-14) with:

```rust
/// App metadata the React shell needs at startup.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub locale: String,
    pub version: String,
    pub background_supported: bool,
}
```

Replace `get_bootstrap_state` (lines ~188-198) with:

```rust
#[tauri::command]
pub async fn get_app_info() -> Result<AppInfo, String> {
    Ok(AppInfo {
        locale: normalize_locale(&system_locale()).to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        background_supported: cfg!(windows),
    })
}

/// Bundled third-party notices. Serialized only when the About panel requests them.
#[tauri::command]
pub async fn get_license_notices() -> Result<String, String> {
    Ok(include_str!("../../../licenses/THIRD-PARTY-NOTICES.txt").to_string())
}
```

Note: returning `String` copies 4.8 MB per open. That is acceptable because the modal
mounts on demand; `&'static str` avoids the copy if the macro accepts it, but do not
spend time on this.

**File:** `crates/app/src/main.rs` (lines ~179-181)

```rust
commands::get_snapshot,
commands::get_app_info,
commands::get_license_notices,
```

Remove `commands::get_bootstrap_state`.

### 3b. Frontend types and invoke wrappers

**File:** `ui/src/api/types.ts` (lines ~569-575)

```ts
export interface AppInfo {
  locale: string;
  version: string;
  backgroundSupported: boolean;
}
```

Delete `BootstrapState`.

**File:** `ui/src/api/commands.ts` (lines ~41-48)

Replace `getBootstrapState` with:

```ts
getAppInfo: () =>
  safeInvoke<AppInfo>('get_app_info', undefined, () => ({
    locale: 'en',
    version: '0.1.0',
    backgroundSupported: false,
  })),

getLicenseNotices: () =>
  safeInvoke<string>(
    'get_license_notices',
    undefined,
    () => 'GameNode Desktop - GPL-3.0-only',
  ),
```

Update the `type` import at the top: `BootstrapState` → `AppInfo`.

### 3c. Queries and hooks

**File:** `ui/src/api/useSnapshot.ts`

Update the type import first: remove `BootstrapState` and add `AppInfo`.

```ts
export const SNAPSHOT_QUERY_KEY = ['snapshot'] as const;
export const APP_INFO_QUERY_KEY = ['app-info'] as const;
export const LICENSE_NOTICES_QUERY_KEY = ['license-notices'] as const;

export function useAppInfo() {
  return useQuery<AppInfo>({
    queryKey: APP_INFO_QUERY_KEY,
    queryFn: () => commands.getAppInfo(),
    staleTime: Infinity,
    gcTime: Infinity,
  });
}

/**
 * Mount only from the notices viewer: this transfers ~4.8 MB over IPC.
 * `gcTime` stays at the client default (5 min) so closing the modal frees it.
 */
export function useLicenseNotices() {
  return useQuery<string>({
    queryKey: LICENSE_NOTICES_QUERY_KEY,
    queryFn: () => commands.getLicenseNotices(),
    staleTime: Infinity,
  });
}
```

Delete `useBootstrapState` and `BOOTSTRAP_QUERY_KEY`. `ui/src/api/index.ts` re-exports
this module, so new symbols are exported automatically.

### 3d. Call sites

Mechanical rename from `useBootstrapState` to `useAppInfo` (and `bootstrap` →
`appInfo` for readability):

| File | Current | New |
| --- | --- | --- |
| `ui/src/App.tsx:4-7, 48, 57` | `useBootstrapState`, `bootstrap.data?.locale` | `useAppInfo`, `appInfo.data?.locale` |
| `ui/src/features/shell/Sidebar.tsx:1-5, 29, 35` | `useBootstrapState`, `bootstrap?.data?.version` | `useAppInfo`, `appInfo?.data?.version` |
| `ui/src/features/shell/SyncExplanationModal.tsx:4, 115, 127` | `useBootstrapState`, `bootstrap?.data?.backgroundSupported` | `useAppInfo`, `appInfo?.data?.backgroundSupported` |
| `ui/src/features/settings/SettingsModal.tsx:11, 70, 260` | `useBootstrapState`, `bootstrap?.version` | `useAppInfo`, `appInfo?.version` |
| `ui/src/features/settings/panels/AboutPanel.tsx:2, 21, 23` | `useBootstrapState`, `bootstrap?.version` | `useAppInfo`, `appInfo?.version` |
| `ui/src/features/settings/LicenseNoticesModal.tsx:19, 42` | `useBootstrapState`, `bootstrap?.licenseNotices` | `useLicenseNotices`, `notices ?? fallback` |

For the license modal, keep the existing fallback string:

```tsx
const { data: notices } = useLicenseNotices();
const text = notices ?? 'GameNode Desktop - GPL-3.0-only';
```

and render `text` where `notices` is rendered today. Because
`SettingsModal.tsx:304-306` mounts the modal only when `licenseModalOpen`, the fetch
happens only when the user opens the viewer. If the modal is ever mounted
unconditionally, pass `enabled: licenseModalOpen` instead.

### 3e. Mock host

**File:** `ui/src/mock/host.ts` (line ~101)

```ts
if (cmd === 'get_snapshot') return copy();
if (cmd === 'get_app_info') {
  return { locale: options.locale ?? 'en', version: 'mock', backgroundSupported: false };
}
if (cmd === 'get_license_notices') return 'Mock UI development session';
```

**File:** `ui/src/mock/host.test.ts`

- Test 1: drop the `get_bootstrap_state` call; read `get_snapshot` directly for the
  auth/library assertions and the deep-copy assertion.
- Test 2: assert the locale via `get_app_info`:
  `expect((await host.invoke('get_app_info') as AppInfo).locale).toBe('pt-BR')`.
- Import `AppInfo` from `../api/types`; keep `Snapshot`.

### 3f. Test updates

**File:** `ui/src/App.integration.test.tsx`

- Import `AppInfo` instead of `BootstrapState`.
- Fake `invoke`:

```ts
if (cmd === 'get_snapshot') return current;
if (cmd === 'get_app_info') {
  return { locale, version: '9.9.9', backgroundSupported: false } satisfies AppInfo;
}
if (cmd === 'get_license_notices') return 'GPL-3.0-only notices';
```

- `mountApp` waits for `get_app_info` instead of `get_bootstrap_state`.
- Rename the locale test at ~line 271 to
  `adopts the host locale (pt-BR) from get_app_info`.

**File:** `ui/src/features/settings/SettingsModal.test.tsx`

- Replace the `BootstrapState` import with `AppInfo` and import
  `APP_INFO_QUERY_KEY`, `LICENSE_NOTICES_QUERY_KEY`.
- Change the helper to seed both entries:

```ts
function createTestQueryClient(
  snapshot?: Partial<Snapshot>,
  appInfo?: Partial<AppInfo>,
  licenseNotices = 'GameNode Desktop - Full offline license notices text',
) {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  // ...existing fullSnapshot/fullAppInfo construction...
  const fullAppInfo: AppInfo = {
    locale: 'en',
    version: '0.2.0',
    backgroundSupported: false,
    ...appInfo,
  };
  qc.setQueryData(SNAPSHOT_QUERY_KEY, fullSnapshot);
  qc.setQueryData(APP_INFO_QUERY_KEY, fullAppInfo);
  qc.setQueryData(LICENSE_NOTICES_QUERY_KEY, licenseNotices);
  return qc;
}
```

- `renderSettings(locale, snapshot, appInfo?, licenseNotices?)` forwards the new
  parameter.
- The license test at ~line 345 currently passes `{ licenseNotices: '...' }` as the
  third argument; change it to
  `renderSettings('en', undefined, undefined, 'Copyright 2026 GameNode Desktop Full License Text')`.
- Other `renderSettings` calls that pass only `(locale, snapshot)` are unchanged.

### 3g. Acceptance for Task 3

- `rg -n "BootstrapState|get_bootstrap_state|useBootstrapState|BOOTSTRAP_QUERY_KEY" ui/src crates/app`
  and `rg -n "licenseNotices" ui/src` return nothing.
- The notices text still renders in the license modal (unit test + manual mock run).
- Startup no longer transfers the notices. In mock mode, open Settings → About and
  confirm the notices appear only after "Show license notices" is clicked. In Rust,
  `get_app_info` returns `AppInfo` (no notices field); `get_license_notices` is the
  only command that touches `THIRD-PARTY-NOTICES.txt`.
- `wc -c licenses/THIRD-PARTY-NOTICES.txt` stays ~4.8 MB; it is now only read on demand.

---

## Task 4 (P1) — Attach the event bridge before the first snapshot fetch

**Problem:** children's query effects run before `App`'s subscription effect. Today
`get_snapshot` can start before `listen('engine:snapshot')` is registered. A snapshot
emitted in that window is lost, and the older command reply can overwrite a newer
event that arrives while the fetch is in flight. The UI then shows stale state until
the next engine change — which may be a long time on an idle screen.

**File:** `ui/src/api/useSnapshot.ts`

1. Add a readiness context (default `true` so isolated renders/tests still fetch):

> Add `/// <reference types="vite/client" />` as the first line of
> `ui/src/api/useSnapshot.ts` (as `ui/src/api/mock.ts` does) so the
> `import.meta.env.DEV` checks below typecheck under `tsc -b`.

```tsx
import {
  createContext,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from 'react';

const SnapshotStreamContext = createContext(true);

/** Attaches the engine bridge before any snapshot query runs. */
export function SnapshotStreamProvider({ children }: { children: ReactNode }) {
  const ready = useEngineSnapshotSubscription();
  return (
    <SnapshotStreamContext.Provider value={ready}>
      {children}
    </SnapshotStreamContext.Provider>
  );
}
```

2. Make `useEngineSnapshotSubscription` return `ready` and log failures in Task 5's
   style. Set `ready` in `finally` so a failed subscription does not wedge the app:

```tsx
export function useEngineSnapshotSubscription(): boolean {
  const queryClient = useQueryClient();
  const [ready, setReady] = useState(false);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;
    subscribeToEngineSnapshots(queryClient)
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch((error) => {
        if (import.meta.env.DEV) {
          console.error('[snapshot] failed to subscribe to engine:snapshot', error);
        }
      })
      .finally(() => {
        if (active) setReady(true);
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [queryClient]);

  return ready;
}
```

3. Gate the fetch and ignore a stale command reply:

```tsx
export function useSnapshot<TSelected = Snapshot>(select?: (snapshot: Snapshot) => TSelected) {
  const queryClient = useQueryClient();
  const streamReady = useContext(SnapshotStreamContext);

  const query = useQuery({
    queryKey: SNAPSHOT_QUERY_KEY,
    queryFn: async () => {
      const startedAt = Date.now();
      const snapshot = await commands.getSnapshot();
      // An engine:snapshot landed while this IPC call was in flight: it is newer.
      const updatedAt = queryClient.getQueryState(SNAPSHOT_QUERY_KEY)?.dataUpdatedAt ?? 0;
      return updatedAt > startedAt
        ? (queryClient.getQueryData<Snapshot>(SNAPSHOT_QUERY_KEY) ?? snapshot)
        : snapshot;
    },
    staleTime: Infinity,
    gcTime: Infinity,
    placeholderData: DEFAULT_SNAPSHOT,
    select,
    enabled: streamReady,
  });

  // ...existing return unchanged
}
```

**File:** `ui/src/AppProviders.tsx`

```tsx
import { SnapshotStreamProvider } from './api/useSnapshot';

// inside AppProviders:
<QueryClientProvider client={client}>
  <SnapshotStreamProvider>
    <I18nProvider initialLocale={initialLocale}>{children}</I18nProvider>
  </SnapshotStreamProvider>
</QueryClientProvider>
```

**File:** `ui/src/App.tsx`

Remove the `useEngineSnapshotSubscription` import and its call; the provider owns it
now.

**Tests:** extend `ui/src/App.integration.test.tsx`:

- Record order in the existing mocks: push `'listen'` in the `listen` mock and
  `` `invoke:${cmd}` `` in the `invoke` mock.
- In `mountApp`, after the app-info wait, also wait for one `get_snapshot` call and
  assert `order.indexOf('listen') < order.indexOf('invoke:get_snapshot')`.
- Add a stale-reply test: start a deferred `get_snapshot`, publish a newer snapshot
  via `publish()`, then resolve the deferred promise with the old snapshot; assert the
  rendered title stays the newer one.

Keep it deterministic — no sleeps. Use `waitFor` only.

**Known residual risk:** if you skip the `updatedAt` guard, an in-flight reply can
still overwrite a newer event. Document any deviation in the PR body.

---

## Task 5 (P1) — Make failures visible in development

**File:** `ui/src/api/useSnapshot.ts`

- In `useSnapshot`, log a fetch failure without changing the public return shape or
  the rendering fallback:

```tsx
useEffect(() => {
  if (query.error && import.meta.env.DEV) {
    console.error('[snapshot] get_snapshot failed', query.error);
  }
}, [query.error]);
```

- In `subscribeToEngineSnapshots`'s caller (the `useEngineSnapshotSubscription`
  catch shown in Task 4), log only when running inside Tauri. Export the existing
  helper from `ui/src/api/commands.ts`:

  ```ts
  export function isTauriRuntime(): boolean { /* existing body */ }
  ```

  and gate the log with `if (isTauriRuntime())`. A plain browser without the event
  mock is expected to fail and should stay quiet.

**Why:** today the snapshot query's error is discarded and `data` silently falls back
to the default snapshot, which looks identical to "signed out". These logs are
dev-only and cost nothing in release builds.

---

## Task 6 (P2, optional) — Centralize query options and align tests

Only do this if the earlier tasks are green; it is cleanup, not a fix.

**New file:** `ui/src/api/queries.ts` (or keep everything in `useSnapshot.ts` if you
prefer one module). Put the key constants here and import them into
`useSnapshot.ts`; do not import `queries.ts` from `useSnapshot.ts` in both
directions, or you create a cycle. Re-export the keys from `useSnapshot.ts` so the
`ui/src/api` public surface stays the same for tests and features:

```ts
// ui/src/api/queries.ts
import { queryOptions } from '@tanstack/react-query';
import { commands } from './commands';

export const SNAPSHOT_QUERY_KEY = ['snapshot'] as const;
export const APP_INFO_QUERY_KEY = ['app-info'] as const;
export const LICENSE_NOTICES_QUERY_KEY = ['license-notices'] as const;

export const appInfoQueryOptions = queryOptions({
  queryKey: APP_INFO_QUERY_KEY,
  queryFn: () => commands.getAppInfo(),
  staleTime: Infinity,
  gcTime: Infinity,
});

export const licenseNoticesQueryOptions = queryOptions({
  queryKey: LICENSE_NOTICES_QUERY_KEY,
  queryFn: () => commands.getLicenseNotices(),
  staleTime: Infinity,
});
```

```ts
// ui/src/api/useSnapshot.ts
import {
  APP_INFO_QUERY_KEY,
  LICENSE_NOTICES_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  appInfoQueryOptions,
  licenseNoticesQueryOptions,
} from './queries';

export { APP_INFO_QUERY_KEY, LICENSE_NOTICES_QUERY_KEY, SNAPSHOT_QUERY_KEY } from './queries';
```

Then `useAppInfo` becomes `useQuery({ ...appInfoQueryOptions })` and
`useLicenseNotices` becomes `useQuery({ ...licenseNoticesQueryOptions })`.

Do **not** move the snapshot query's `queryFn` here unless you also pass the
`QueryClient` into a factory; it needs the stale-write guard from Task 4. Keep the
snapshot options inline.

Then swap the hand-rolled clients that exercise `useSnapshot`/`useAppInfo` for
`createAppQueryClient()` where practical:
`ui/src/features/settings/SettingsModal.test.tsx`,
`ui/src/features/dialogs/dialogs.test.tsx`, `ui/src/features/auth/auth.test.tsx`,
`ui/src/features/library/library.test.tsx`, `ui/src/features/tools/GameToolsModal.test.tsx`,
`ui/src/features/drm-free/drmFree.test.tsx`,
`ui/src/features/shell/shell.test.tsx`,
`ui/src/features/drm-free/useLocalGameErrorToast.test.tsx`,
`ui/src/api/useSnapshot.test.tsx`, `ui/src/mock/bridge.test.ts`. Tests that only
`setQueryData` are unaffected; do not churn the ones that do not touch these hooks.

---

## Verification

Run from the repository root:

```sh
pnpm --dir ui test --run
pnpm --dir ui build
cargo check --locked -p gamenode-app
cargo test --locked -p gamenode-app
```

Symbol checks:

```sh
rg -n "BootstrapState|get_bootstrap_state|useBootstrapState|BOOTSTRAP_QUERY_KEY" ui/src crates/app
rg -n "useMutation|mutate\(" ui/src
rg -n "licenseNotices|license_notices" ui/src crates/app/src/commands.rs
```

Expect no output for the first two. The third should only show the Rust
`get_license_notices` command (its name contains `license_notices`); no frontend
`licenseNotices` identifier should survive.

Manual smoke test (signed-in mock):

```sh
VITE_UI_MOCK=1 pnpm --dir ui dev
```

1. App loads and shows the mock library with the version in the sidebar.
2. Settings → About → "Show license notices" opens and shows the mock notices.
3. Settings → About → Escape closes only the notices modal.
4. No console error about the engine bridge in mock mode.

## Acceptance criteria

- [ ] No `useMutation` or `mutate(` call remains in `ui/src`.
- [ ] `get_bootstrap_state` and `BootstrapState` are gone; `get_app_info` and
      `get_license_notices` are registered in `main.rs` and wrapped in `commands.ts`.
- [ ] Opening the app does not serialize the license notices; opening the viewer does.
- [ ] The snapshot listener is registered before the first `get_snapshot` call.
- [ ] A snapshot event that arrives during the initial fetch is not overwritten by the
      older reply.
- [ ] Snapshot fetch/subscription errors are logged in development, and the UI still
      falls back to the default snapshot rather than crashing.
- [ ] `pnpm --dir ui test --run` and `cargo test --locked -p gamenode-app` pass.
- [ ] No new dependencies in `ui/package.json`.

## Risks and notes

- **4.8 MB string:** returning `String` allocates a copy per open. The modal mounts on
  demand, so this is a one-off; do not add caching layers in Rust for it.
- **Event ordering:** the engine throttles at 100 ms and drops unchanged snapshots
  (`crates/app/src/commands.rs:49, 60-140`). Do not replace the bridge with polling or
  `invalidateQueries`; that would multiply full-state serializations.
- **Offline behavior:** `networkMode: 'always'` is deliberate. If a future query talks
  to a remote HTTP API, override the mode on that query.
- **`staleTime: Infinity`:** the snapshot, app info and notices never go stale on
  their own. Keep it; the engine/events are the invalidation mechanism.
- **Historical docs:** `docs/tauri-v2-migration-plan.md` describes the original plan
  (per-slice keys, mutations for every action). Leave it as history; this hand-off is
  the current contract.

## Suggested commit split

If asked to commit, follow `CONTRIBUTING.md`; keep behavior changes separate:

1. `perf(ui): load license notices on demand` — Task 3 (Rust + frontend + tests).
2. `refactor(ui): remove unused snapshot mutation hooks` — Task 2.
3. `fix(ui): keep engine snapshots authoritative over the initial fetch` — Tasks 4-5.
4. `refactor(ui): centralize query defaults and options` — Tasks 1 and 6 (if done).

Do not mix `docs/handoff-tanstack-query.md` with application commits unless the user
asks; if the document is committed, use `docs(ui): add TanStack Query hand-off`.
