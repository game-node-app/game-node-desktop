import { mockEnabled } from './mock';
import type { PortPlatform, PortsView } from './ports';

export type Provider = 'google' | 'discord' | 'twitter';

export interface User {
  id: string;
  username?: string | null;
  email?: string | null;
}

export type AuthView =
  | 'starting'
  | 'signedOut'
  | { pending: { provider: Provider; awaitingBrowser: boolean } }
  | 'verifying'
  | { signedIn: { user: User } }
  | 'restoreFailed'
  | 'signingOut';

export type Notice =
  | 'browserLaunchFailed'
  | 'signInUnavailable'
  | 'providerRejected'
  | 'attemptExpired'
  | 'signInCancelled'
  | 'signInRejected'
  | 'connectionProblem'
  | 'sessionRejected'
  | 'credentialUnreadable'
  | 'persistFailed'
  | 'signOutNotConfirmed'
  | 'credentialDeleteFailed';

export type StorageMethod =
  | 'windowsCredentialManager'
  | 'secretService'
  | 'privateFile';

export interface WindowGeometry {
  width: number;
  height: number;
  x?: number | null;
  y?: number | null;
  maximized: boolean;
}

export interface Settings {
  window?: WindowGeometry | null;
}

export type PlaytimeSource = 'internal' | 'store';

export interface PlaytimeView {
  seconds?: number | null;
  source: PlaytimeSource;
  /** Unix seconds from the selected source; null when that source has no date. */
  lastPlayedAt?: number | null;
}

export interface SyncPreferences {
  enabled: boolean;
  explained: boolean;
}

export interface TrackingView {
  title: string;
  available: boolean;
  running: string[];
}

export type MatchSource = 'automatic' | 'chosen';

export type MatchStatus =
  | 'unmatched'
  | 'ambiguous'
  | 'noGame'
  | { matched: { gameId: number; source: MatchSource } };

export type EntryStatus =
  | 'playing'
  | 'finished'
  | 'planned'
  | 'dropped'
  | 'ongoing';

export type Membership =
  | 'notChecked'
  | 'checking'
  | 'unavailable'
  | 'notInLibrary'
  | { inLibrary: EntryStatus | null };

export interface Row {
  playtime: PlaytimeView;
  running: boolean;
  key: string;
  title: string;
  detectedName: string;
  matchedName?: string | null;
  provider: string;
  providerName: string;
  installed: boolean;
  owned: boolean;
  familyShared: boolean;
  status: MatchStatus;
  coverFile?: string | null;
  /** The matched game's release year; unknown for unmatched entries. */
  releaseYear?: number | null;
  /** Unix seconds of the scan that first detected the entry. */
  addedAt: number;
  /** Whether the matched game is one of the account's favorites. */
  isFavorite: boolean;
}

export interface DetailView {
  running: boolean;
  row: Row;
  installDir: string;
  releaseYear?: number | null;
  hasArtwork: boolean;
  membership: Membership;
}

export type EmptyState =
  | { noGames: { stores: string[] } }
  | { noInstalled: { provider: string | null } }
  | 'nothingMatches';

export type ViewMode = 'grid' | 'list';

export type SortKey =
  | 'title'
  | 'provider'
  | 'matchStatus'
  | 'releaseYear'
  | 'playtime'
  | 'lastPlayed'
  | 'dateAdded';
export type SortDirection = 'ascending' | 'descending';

/** The direction that best answers the question each sort key asks. */
export const DEFAULT_SORT_DIRECTION: Record<SortKey, SortDirection> = {
  title: 'ascending',
  provider: 'ascending',
  matchStatus: 'ascending',
  releaseYear: 'descending',
  playtime: 'descending',
  lastPlayed: 'descending',
  dateAdded: 'descending',
};

export function defaultSortDirection(key: SortKey): SortDirection {
  return DEFAULT_SORT_DIRECTION[key];
}

export interface Sort {
  key: SortKey;
  direction: SortDirection;
}

export type SidebarSelection = 'all' | { provider: string };

export interface AvailabilityFilter {
  installed: boolean;
  owned: boolean;
  family: boolean;
}

export type MatchFilter = 'any' | 'matched' | 'ambiguous' | 'unmatched' | 'noGame';
export type PlaytimeFilter = 'any' | 'unplayed' | 'played' | 'atLeast10h' | 'atLeast100h';
export type LastPlayedFilter = 'any' | 'last7Days' | 'last30Days' | 'lastYear';
export type ReleaseFilter = 'any' | 'earlier' | 'unknown' | { decade: number };
export type AddedFilter = 'any' | 'last7Days' | 'last30Days' | 'last90Days' | 'lastYear';

/** Every toolbar facet. Facets combine with AND; availability facts are alternatives. */
export interface LibraryFilters {
  availability: AvailabilityFilter;
  runningOnly: boolean;
  matchState: MatchFilter;
  playtime: PlaytimeFilter;
  lastPlayed: LastPlayedFilter;
  release: ReleaseFilter;
  added: AddedFilter;
  favoritesOnly: boolean;
}

export function createDefaultFilters(): LibraryFilters {
  return {
    availability: { installed: false, owned: false, family: false },
    runningOnly: false,
    matchState: 'any',
    playtime: 'any',
    lastPlayed: 'any',
    release: 'any',
    added: 'any',
    favoritesOnly: false,
  };
}

export function filtersAreDefault(filters: LibraryFilters): boolean {
  return (
    !filters.availability.installed &&
    !filters.availability.owned &&
    !filters.availability.family &&
    !filters.runningOnly &&
    filters.matchState === 'any' &&
    filters.playtime === 'any' &&
    filters.lastPlayed === 'any' &&
    filters.release === 'any' &&
    filters.added === 'any' &&
    !filters.favoritesOnly
  );
}

export interface LibraryLayout {
  view: ViewMode;
  sort: Sort;
  filter: SidebarSelection;
  filters: LibraryFilters;
  sidebarWidth: number;
  detailWidth: number;
  detailOpen: boolean;
  /** Optional so fixtures written before the collapse flag existed keep compiling. */
  sidebarCollapsed?: boolean;
}

export type Access = 'unverified' | 'offline' | 'verified';

export interface ProviderCount {
  id: string;
  name: string;
  count: number;
  installedCount: number;
  ownedCount: number;
  familyCount: number;
}

export type Reason =
  | 'running'
  | 'nothingSelected'
  | 'multipleSelected'
  | 'noLaunchRule'
  | 'noInstallRule'
  | 'missingField'
  | 'needsVerifiedSession'
  | 'offline'
  | 'notMatched'
  | 'noMatchedGames'
  | 'checkingMembership'
  | 'membershipUnavailable'
  | { alreadyInLibrary: EntryStatus | null };

export type Availability = 'available' | { disabled: Reason };

export interface Actions {
  selected: number;
  play: Availability;
  install: Availability;
  matchGame: Availability;
  addToGamenode: Availability;
  changeMatch: boolean;
}

export interface MatchOption {
  gameId: number;
  name: string;
  releaseYear?: number | null;
  coverUrl?: string | null;
  coverFile?: string | null;
  current: boolean;
}

export type SearchFailure =
  | { blocked: Reason }
  | 'rejected'
  | 'failed';

export interface MatchDialogView {
  entryKey: string;
  detectedName: string;
  providerName: string;
  query: string;
  searching: boolean;
  candidates: MatchOption[];
  results: MatchOption[];
  noResults: boolean;
  error?: SearchFailure | null;
  searchBlocked?: Reason | null;
  current: MatchStatus;
  canClear: boolean;
}

export interface AddTarget {
  entryKey: string;
  title: string;
  matched: boolean;
}

export interface CollectionOption {
  id: string;
  name: string;
  defaultStatus?: EntryStatus | null;
  picked: boolean;
}

export type ApiError = 'unauthorized' | 'failed';

export type CollectionsView =
  | 'loading'
  | { failed: ApiError }
  | { loaded: CollectionOption[] };

export interface AddProgress {
  done: number;
  total: number;
}

export type AddOutcome =
  | 'added'
  | 'alreadyPresent'
  | 'skipped'
  | 'failed';

export interface AddItem {
  entryKey: string;
  title: string;
  outcome: AddOutcome;
}

export interface AddSummary {
  items: AddItem[];
  added: number;
  alreadyPresent: number;
  skipped: number;
  failed: number;
}

export interface AddDialogView {
  targets: AddTarget[];
  editingExisting: boolean;
  status: EntryStatus;
  appliedStatus: EntryStatus;
  appliedFrom?: string | null;
  collections: CollectionsView;
  progress?: AddProgress | null;
  error?: ApiError | null;
  canConfirm: boolean;
  blocked?: Reason | null;
  summary?: AddSummary | null;
}

export interface LibraryView {
  generation: number;
  layout: LibraryLayout;
  search: string;
  access: Access;
  total: number;
  installedTotal: number;
  providers: ProviderCount[];
  rows: Row[];
  scanning: boolean;
  failedProviders: string[];
  selected: string[];
  focused?: string | null;
  actions: Actions;
  detail?: DetailView | null;
  empty?: EmptyState | null;
  matchDialog?: MatchDialogView | null;
  addDialog?: AddDialogView | null;
}

export type LaunchKind = 'play' | 'install';

export type LaunchError = 'missingHandler' | 'unsupported' | 'failed';

export type LaunchState = 'opening' | { failed: LaunchError };

export interface LaunchStatus {
  entryKey: string;
  kind: LaunchKind;
  state: LaunchState;
}

export interface Registration {
  id: string;
  title: string;
  executable: string;
  fileIdentity: string;
  root: string;
  workingDirectory: string;
  arguments: string[];
  gameplay: string[];
  launchers: string[];
  revision: number;
  removed: boolean;
  rediscoveryAllowed?: boolean;
  transferredFrom?: string | null;
  available: boolean;
}

export type EvidenceKind =
  | 'product'
  | 'description'
  | 'executable'
  | 'folder'
  | 'manifest'
  | 'externalId';

export interface Evidence {
  kind: EvidenceKind;
  value: string;
  strong: boolean;
}

export interface Inference {
  installation: string;
  revision: number;
  title: string;
  evidence: Evidence[];
  gameId?: number | null;
  account: string;
}

export interface Candidate {
  fingerprint: string;
  watch: string;
  root: string;
  title: string;
  targets: string[];
  selectedTarget?: string | null;
  evidence: Evidence[];
  ignored: boolean;
  incomplete: boolean;
}

export interface WatchRoot {
  id: string;
  path: string;
  enabled: boolean;
}

export type SaveSupport =
  | 'supported'
  | 'needsSelection'
  | 'unsupported'
  | 'noSaves';

export interface SaveProfile {
  id: string;
  installation: string;
  revision: number;
  title: string;
  manifestHash: string;
  definition: string;
  files: string[];
  registry: string[];
  support: SaveSupport;
  automatic?: boolean | null;
}

export type JobState =
  | 'queued'
  | 'running'
  | 'unchanged'
  | 'successful'
  | 'partial'
  | 'failed'
  | 'cancelled';

export interface BackupJob {
  id: string;
  installation: string;
  profile: string;
  revision: number;
  playPeriod?: string | null;
  destination: string;
  state: JobState;
  attempts: number;
  readyAt: number;
  backupId?: string | null;
  error?: string | null;
}

export interface BackupSettings {
  automatic: boolean;
  destination: string;
}

export interface BackupVersion {
  installation: string;
  profile: string;
  destination: string;
  id: string;
  label: string;
  bytes: number;
}

export type BackupOperation =
  | 'preview'
  | 'backup'
  | 'history'
  | { restorePreview: { backup: string } }
  | { restore: { backup: string } };

export interface BackupRequest {
  jobId?: string | null;
  profile: SaveProfile;
  installation: Registration;
  destination: string;
  operation: BackupOperation;
}

export interface RestorePlan {
  request: BackupRequest;
  targets: string[];
}

export interface PendingClaim {
  registration: Registration;
  storeKey: string;
  storeTitle: string;
}

export interface LocalView {
  pendingClaim?: PendingClaim | null;
  inferences: Inference[];
  recentlyAdded: string[];
  discoveryIncomplete: string[];
  discoveryScanning: boolean;
  discoveryCompleted: boolean;
  discoveryError?: string | null;
  watches: WatchRoot[];
  reviewSelected: string[];
  versions: BackupVersion[];
  restore?: RestorePlan | null;
  busy: boolean;
  error?: string | null;
  registrations: Registration[];
  candidates: Candidate[];
  profiles: SaveProfile[];
  jobs: BackupJob[];
  settings: BackupSettings;
}

export type ToolChannel = 'stable' | 'nightly';
export type ToolInstallState =
  | 'installing'
  | 'installed'
  | 'updating'
  | 'uninstalling';

export type ProxyDllName =
  | 'dxgi'
  | 'winmm'
  | 'version'
  | 'dbghelp'
  | 'd3d12'
  | 'wininet'
  | 'winhttp';

export interface GameToolView {
  toolId: string;
  name: string;
  channel: ToolChannel;
  version: string;
  proxyDll?: string | null;
  targetExe: string;
  state: ToolInstallState;
  summary: string;
}

export interface TargetCandidate {
  relativeExe: string;
  label: string;
  architecture: string;
  preferred: boolean;
}

// Mirrors `AntiCheatSystem` in crates/core/src/tools/anticheat.rs (camelCase).
export type AntiCheatSystem = 'easyAntiCheat' | 'battlEye' | 'blocklisted';

// Mirrors `AntiCheatReport`: an empty `detected` list means nothing was found.
export interface AntiCheatReport {
  detected: AntiCheatSystem[];
  evidence: string[];
}

export interface TargetInspection {
  entryKey: string;
  installDir: string;
  candidates: TargetCandidate[];
  selectedExe?: string | null;
  anticheat: AntiCheatReport;
}

export interface ToolRelease {
  toolId: string;
  channel: ToolChannel;
  version: string;
  tagName: string;
  name: string;
  publishedAt: string;
  downloadUrl: string;
  assetName: string;
  sizeBytes: number;
  sha256?: string | null;
  cachedPath?: string | null;
}

export type ToolOperationStatus =
  | { installing: { progressPercent?: number | null; message: string } }
  | { updating: { progressPercent?: number | null; message: string } }
  | 'uninstalling'
  | { failed: { message: string } };

export interface ToolModalView {
  entryKey: string;
  title: string;
  installDir: string;
  inspection?: TargetInspection | null;
  tools: GameToolView[];
  activeToolId: string;
  selectedChannel: ToolChannel;
  availableVersions: ToolRelease[];
  selectedVersion?: string | null;
  selectedProxy: ProxyDllName;
  anticheatConfirmed: boolean;
  operation?: ToolOperationStatus | null;
}

export interface ToolCatalogItemView {
  id: string;
  name: string;
  summary: string;
  license: string;
  sourceUrl: string;
}

export interface ToolsView {
  installedByEntry: Record<string, GameToolView[]>;
  modal?: ToolModalView | null;
  totalCacheBytes: number;
  totalCacheArchives: number;
  catalog: ToolCatalogItemView[];
}

export type CatalogError =
  | 'unreachable'
  | 'rateLimited'
  | 'server'
  | 'expired'
  | 'privacy'
  | 'changed'
  | 'invalidCookies'
  | 'familyUnavailable';

export type StoreAccountState =
  | 'notConnected'
  | 'checking'
  | 'signingIn'
  | 'connected'
  | 'expired'
  | 'unreachable'
  | 'credentialError';

export interface StoreAccountView {
  provider: string;
  state: StoreAccountState;
  accountId: string | null;
  accountName: string | null;
  ownedCount: number;
  familyCount: number;
  installedCount: number;
  lastCheckedAt: number | null;
  /** Last successful catalog sync in this run; the connected toast waits for it. */
  lastSyncedAt?: number | null;
  storage: StorageMethod | null;
  persistFailed: boolean;
  catalogError: CatalogError | null;
  familyError: CatalogError | null;
  canSignIn: boolean;
  canCancelSignIn: boolean;
  canFocusSignIn: boolean;
  canSignOut: boolean;
  canCheck: boolean;
  canRemoveCredential: boolean;
}

export interface PluginInfo {
  providerId: string;
  pluginId: string;
  name: string;
  source: 'official' | 'community';
  status: 'installed' | 'missing' | 'unavailable' | 'failed' | 'incompatible';
  capabilities: string[];
  session: 'active' | 'stopped';
  nativeAccess: string[];
}

export interface Snapshot {
  localGames: LocalView;
  playtimeError: boolean;
  tracking: TrackingView;
  sync: SyncPreferences;
  playtimeSource: PlaytimeSource;
  auth: AuthView;
  notice?: Notice | null;
  storage: StorageMethod;
  settings: Settings;
  library?: LibraryView | null;
  launch?: LaunchStatus | null;
  tools: ToolsView;
  storeAccounts: StoreAccountView[];
  /** PC ports catalog, provenance and install jobs. Always present; empty until opened. */
  ports: PortsView;
}

export interface AppInfo {
  locale: string;
  version: string;
  backgroundSupported: boolean;
  /**
   * Platform reported by the host. Optional because older hosts do not send it; the UI then
   * falls back to the user agent. The mock reports `windows` so PC ports can be developed
   * on any machine.
   */
  platform?: PortPlatform | null;
  logDirectory?: string | null;
}

export type AuthInput =
  | 'start'
  | { startSignIn: Provider }
  | 'cancelSignIn'
  | { callbackReceived: string }
  | 'retryRestore'
  | 'signOut'
  | 'tick'
  | 'sessionRejected';

export type SelectMode = 'replace' | 'toggle' | 'range';
export type Direction = 'up' | 'down' | 'left' | 'right' | 'home' | 'end';

export type LibraryInput =
  | 'scanRequested'
  | 'openMatch'
  | 'closeMatch'
  | { setMatchQuery: string }
  | 'searchMatch'
  | { pickMatch: { gameId: number; now: number } }
  | { markNoGame: { now: number } }
  | 'clearMatch'
  | { select: { key: string; mode: SelectMode } }
  | { move: { direction: Direction; extend: boolean } }
  | { setColumns: number }
  | { setView: ViewMode }
  | { setFilter: SidebarSelection }
  | { setSearch: string }
  | { setSort: Sort }
  | { setFilters: LibraryFilters }
  | { setFavorite: { key: string; favorite: boolean; now: number } }
  | { setSidebarWidth: number }
  | { setSidebarCollapsed: boolean }
  | { setDetailWidth: number }
  | { setDetailOpen: boolean }
  | 'openDetails'
  | 'refreshMembership'
  | 'openAdd'
  | 'closeAdd'
  | { setAddStatus: EntryStatus }
  | { toggleAddCollection: string }
  | 'retryCollections'
  | 'confirmAdd'
  | { removeEntry: { key: string; allowRediscovery: boolean } }
  | 'resetExcluded';

export function getMatchedGameId(status: MatchStatus): number | null {
  if (typeof status === 'object' && 'matched' in status) {
    return status.matched.gameId;
  }
  return null;
}

/**
 * Base of the `gamenode-asset://` custom protocol as the webview loads it. WebView2 on Windows
 * serves custom schemes only as `http://<scheme>.localhost`, so the bare scheme never loads
 * there; other platforms use it directly. Same rule as Tauri's own `convertFileSrc`.
 */
export function assetProtocolBase(): string {
  const onWindows = typeof navigator !== 'undefined' && navigator.userAgent.includes('Windows');
  return onWindows ? 'http://gamenode-asset.localhost' : 'gamenode-asset://localhost';
}

export function coverAssetUrl(coverFile: string | null | undefined): string | null {
  if (!coverFile) return null;
  if (mockEnabled && /^mock-\d+\.svg$/.test(coverFile)) return `/mock-covers/${coverFile}`;
  return `${assetProtocolBase()}/cover/${encodeURIComponent(coverFile)}`;
}

export function artworkAssetUrl(gameId: number | null | undefined, rev?: number | string): string | null {
  if (gameId == null) return null;
  if (mockEnabled) return `/mock-covers/mock-${(gameId - 1) % 12}.svg`;
  const suffix = rev !== undefined ? `?rev=${encodeURIComponent(String(rev))}` : '';
  return `${assetProtocolBase()}/artwork/${gameId}${suffix}`;
}

/**
 * The empty PC ports snapshot used before the host delivers its first payload and by the
 * web preview. The factory lives here, not in `api/ports.ts`, so `types.ts` never needs a
 * runtime import of the ports module (which would create an import cycle through
 * `useSnapshot.ts`).
 */
export function createDefaultPortsView(): PortsView {
  return {
    catalog: {
      state: 'idle',
      fetchedAt: null,
      entryCount: 0,
      error: null,
    },
    entries: [],
    provenance: {},
    jobs: [],
    unseenCount: 0,
    preferences: {
      view: 'grid',
      filters: {
        consoles: [],
        guided: false,
        verified: false,
        installed: false,
      },
      lastParentFolder: null,
    },
  };
}

export function createDefaultSnapshot(): Snapshot {
  return {
    localGames: {
      pendingClaim: null,
      inferences: [],
      recentlyAdded: [],
      discoveryIncomplete: [],
      discoveryScanning: false,
      discoveryCompleted: false,
      discoveryError: null,
      watches: [],
      reviewSelected: [],
      versions: [],
      restore: null,
      busy: false,
      error: null,
      registrations: [],
      candidates: [],
      profiles: [],
      jobs: [],
      settings: {
        automatic: true,
        destination: '',
      },
    },
    playtimeError: false,
    tracking: {
      title: '',
      available: false,
      running: [],
    },
    sync: {
      enabled: true,
      explained: true,
    },
    playtimeSource: 'internal',
    auth: 'signedOut',
    notice: null,
    storage: 'secretService',
    settings: {
      window: null,
    },
    library: null,
    launch: null,
    tools: {
      installedByEntry: {},
      modal: null,
      totalCacheBytes: 0,
      totalCacheArchives: 0,
      catalog: [],
    },
    storeAccounts: [],
    ports: createDefaultPortsView(),
  };
}
