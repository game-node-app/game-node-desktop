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
  /** Running library keys, independent of filters and hidden state. */
  running: string[];
  /**
   * Unix second each running key's current execution began. The status bar
   * counts from this value, never from when the view mounted.
   */
  sessionStartedAt: Record<string, number>;
}

export type MatchSource = 'automatic' | 'chosen';

/**
 * How an entry maps to a GameNode game. The engine sends `ambiguous` as an externally tagged
 * object with its candidate ids; the string form is kept for the mock and older fixtures.
 */
export type MatchStatus =
  | 'unmatched'
  | 'ambiguous'
  | 'noGame'
  | { ambiguous: { candidates: number[] } }
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

export interface RowGameNode {
  /** null when GameNode sent a status this build does not know. */
  status: EntryStatus | null;
  collectionIds: string[];
  entryId: string;
}

export interface Row {
  playtime: PlaytimeView;
  running: boolean;
  /** Unix second the current execution began; null/absent when stopped or unknown. */
  sessionStartedAt?: number | null;
  key: string;
  title: string;
  detectedName: string;
  matchedName: string | null;
  provider: string;
  providerName: string;
  installed: boolean;
  owned: boolean;
  shared: boolean;
  status: MatchStatus;
  coverFile: string | null;
  /** The matched game's release year; unknown for unmatched entries. */
  releaseYear: number | null;
  /** Unix seconds of the scan that first detected the entry. */
  addedAt: number;
  /** Whether the matched game is one of the account's favorites. */
  isFavorite: boolean;
  /** Hidden from every scope, count and selection until Show hidden is on. */
  hidden: boolean;
  /** The cached GameNode library entry, when the account library has one. */
  gamenode: RowGameNode | null;
}

export type SuggestionTone = 'success' | 'warning' | 'neutral';

/** One scored match suggestion for an unmatched or ambiguous entry. */
export interface SuggestionView {
  gameId: number;
  title: string;
  year: number | null;
  coverUrl: string | null;
  /** Whole percent, 0–100, as the Rust view sends it. */
  confidence: number;
  tone: SuggestionTone;
}

export type CatalogStatus = 'idle' | 'loading' | 'ready' | 'error' | 'offline';

export interface CatalogResult {
  /** Downloaded images served by the local asset protocol. */
  coverFile?: string | null;
  hasArtwork?: boolean;
  gameId: number;
  title: string;
  year: number | null;
  coverUrl: string | null;
  /** The account library cache has an entry for this game. */
  inLibrary: boolean;
  /** The first local library row matched to this game, when there is one. */
  localKey: string | null;
}

export interface CatalogView {
  query: string;
  status: CatalogStatus;
  results: CatalogResult[];
  /** The search said its answer may be incomplete. */
  incomplete: boolean;
}

export type ScanProgressState = 'waiting' | 'scanning' | 'done' | 'failed';

export interface ScanProgress {
  provider: string;
  name: string;
  state: ScanProgressState;
  /** The provider's reported record count, when its scan succeeded. */
  found: number | null;
}

/** One lightweight command-palette game entry. */
export interface PaletteEntry {
  key: string;
  title: string;
  providerName: string;
  installed: boolean;
  playtimeLabel: string | null;
}

export type AchievementGameState = 'ready' | 'none' | 'failed';

export interface AchievementUnlock {
  id: string;
  name: string;
  unlockedAt?: number | null;
}

export interface AchievementSummary {
  state: AchievementGameState;
  unlocked: number;
  total: number;
  latestUnlock?: AchievementUnlock | null;
  lastSuccessAt?: number | null;
  failure?: CatalogError | null;
}

export interface AchievementRecord {
  id: string;
  name: string;
  description: string;
  hidden: boolean;
  iconUrl: string;
  iconGrayUrl: string;
  unlocked: boolean;
  unlockedAt?: number | null;
  position: number;
}

export type AchievementFilter = 'all' | 'unlocked' | 'locked';

export interface AchievementListView {
  connected: boolean;
  state?: AchievementGameState | null;
  lastSuccessAt?: number | null;
  failure?: CatalogError | null;
  achievements: AchievementRecord[];
}

export interface DetailView {
  running: boolean;
  row: Row;
  installDir: string;
  releaseYear: number | null;
  hasArtwork: boolean;
  membership: Membership;
  achievementBackend: boolean;
  achievements: AchievementSummary | null;
  detectedTarget?: DetectedTargetView | null;
  /** Best-first suggestions for an unmatched or ambiguous entry. */
  suggestions: SuggestionView[];
}

export type DetectedTargetSpec =
  | { uri: string }
  | { executable: { root: string; path: string; arguments: string[]; workingDirectory: string } };

export interface DetectedTargetView {
  spec: DetectedTargetSpec;
  revision: number;
  approved: boolean;
  available: boolean;

}

export interface LaunchRecipeOption {
  pluginId: string;
  publisherFingerprint: string;
  recipeId: string;
  recipeRevision: number;
}

export interface LaunchRecipeSelection {
  entryKey: string;
  ownerKey: string;
  pluginId: string;
  publisherFingerprint: string;
  recipeId: string;
  recipeRevision: number;
  baseTargetRevision: number;
  approvedPlanFingerprint: string;
}

export interface LaunchRecipeChoices {
  targetRevision: number;
  detectedDefaultAvailable: boolean;
  selected: LaunchRecipeSelection | null;
  recipes: LaunchRecipeOption[];
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
  | 'gameNodeStatus'
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
  gameNodeStatus: 'ascending',
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

/**
 * The pre-scope sidebar selection. The input remains for the current sidebar until the
 * redesigned sidebar sends scopes directly; layouts only ever store `scope` and `filters.stores`.
 */
export type SidebarSelection = 'all' | { provider: string };

/** The sidebar scope. A store shortcut is `'all'` plus one value in `filters.stores`. */
export type LibraryScope =
  | 'all'
  | 'installed'
  | 'favorites'
  | 'recent'
  | 'running'
  | { collection: string }
  | { savedView: number };

export type GroupBy =
  | 'none'
  | 'store'
  | 'gameNodeStatus'
  | 'matchState'
  | 'installation'
  | 'releaseDecade';

export interface AvailabilityFilter {
  installed: boolean;
  owned: boolean;
  shared: boolean;
}

export type MatchFilterValue = 'matched' | 'ambiguous' | 'unmatched' | 'noGame';
export type GameNodeStatusFilter =
  | 'playing'
  | 'finished'
  | 'planned'
  | 'ongoing'
  | 'dropped'
  | 'inLibrary'
  | 'notInLibrary';
export type PlaytimeThreshold = 'any' | 'atLeast10h' | 'atLeast100h';
export type PlayedFilter = 'any' | 'played' | 'unplayed';
export type LastPlayedFilter =
  | 'any'
  | 'last7Days'
  | 'last30Days'
  | 'last90Days'
  | 'lastYear'
  | 'olderThanYear';
export type ReleaseFilter = 'any' | 'earlier' | 'unknown' | { decade: number };
export type AddedFilter =
  | 'any'
  | 'last7Days'
  | 'last30Days'
  | 'last90Days'
  | 'lastYear'
  | 'olderThanYear';

/**
 * Every toolbar facet. Facets combine with AND; the values inside availability, stores,
 * matchStates and gamenodeStatus are alternatives within their facet.
 */
export interface LibraryFilters {
  availability: AvailabilityFilter;
  stores: string[];
  matchStates: MatchFilterValue[];
  gamenodeStatus: GameNodeStatusFilter[];
  playtime: PlaytimeThreshold;
  played: PlayedFilter;
  lastPlayed: LastPlayedFilter;
  release: ReleaseFilter;
  added: AddedFilter;
  favoritesOnly: boolean;
  runningOnly: boolean;
  showHidden: boolean;
}

export function createDefaultFilters(): LibraryFilters {
  return {
    availability: { installed: false, owned: false, shared: false },
    stores: [],
    matchStates: [],
    gamenodeStatus: [],
    playtime: 'any',
    played: 'any',
    lastPlayed: 'any',
    release: 'any',
    added: 'any',
    favoritesOnly: false,
    runningOnly: false,
    showHidden: false,
  };
}

export function filtersAreDefault(filters: LibraryFilters): boolean {
  return (
    !filters.availability.installed &&
    !filters.availability.owned &&
    !filters.availability.shared &&
    filters.stores.length === 0 &&
    filters.matchStates.length === 0 &&
    filters.gamenodeStatus.length === 0 &&
    filters.playtime === 'any' &&
    filters.played === 'any' &&
    filters.lastPlayed === 'any' &&
    filters.release === 'any' &&
    filters.added === 'any' &&
    !filters.favoritesOnly &&
    !filters.runningOnly &&
    !filters.showHidden
  );
}

/** The default remembered layout, matching `LibraryLayout::default()` in the engine. */
export function createDefaultLibraryLayout(): LibraryLayout {
  return {
    view: 'grid',
    scope: 'all',
    sort: { key: 'title', direction: 'ascending' },
    filters: createDefaultFilters(),
    group: 'none',
    collapsedGroups: {},
    sidebarWidth: 240,
    detailWidth: 360,
    detailOpen: true,
    sidebarCollapsed: false,
  };
}

export interface LibraryLayout {
  view: ViewMode;
  scope: LibraryScope;
  sort: Sort;
  filters: LibraryFilters;
  group: GroupBy;
  collapsedGroups: Partial<Record<GroupBy, string[]>>;
  sidebarWidth: number;
  detailWidth: number;
  detailOpen: boolean;
  sidebarCollapsed: boolean;
}

export type Access = 'unverified' | 'offline' | 'verified';

export interface ProviderCount {
  id: string;
  name: string;
  count: number;
  installedCount: number;
  ownedCount: number;
  /** Entries shared through the provider's declared shared access and not owned. */
  sharedCount: number;
  /** The provider's declared shared-access label; only labelled providers offer the filter. */
  sharedLabel: string | null;
}

/** Mirrors `StoreCount` in `crates/core/src/library/query.rs`. */
export interface StoreCount {
  id: string;
  name: string;
  count: number;
}

export interface AvailabilityCounts {
  installed: number;
  owned: number;
  shared: number;
}

export interface MatchStateCounts {
  matched: number;
  ambiguous: number;
  unmatched: number;
  noGame: number;
}

export interface GameNodeStatusCounts {
  playing: number;
  finished: number;
  planned: number;
  ongoing: number;
  dropped: number;
  inLibrary: number;
  notInLibrary: number;
  /** False until this account's GameNode library has been read at least once. */
  available: boolean;
}

export interface PlaytimeCounts {
  any: number;
  atLeast10h: number;
  atLeast100h: number;
}

export interface PlayedCounts {
  any: number;
  played: number;
  unplayed: number;
}

export interface LastPlayedCounts {
  any: number;
  last7Days: number;
  last30Days: number;
  last90Days: number;
  lastYear: number;
  olderThanYear: number;
}

/** One release option: only options with rows are listed. */
export interface ReleaseCount {
  value: ReleaseFilter;
  count: number;
}

export interface AddedCounts {
  any: number;
  last7Days: number;
  last30Days: number;
  last90Days: number;
  lastYear: number;
  olderThanYear: number;
}

/** Mirrors `FilterCounts` in `crates/core/src/library/query.rs`. */
export interface FilterCounts {
  stores: StoreCount[];
  availability: AvailabilityCounts;
  matchStates: MatchStateCounts;
  gamenodeStatus: GameNodeStatusCounts;
  playtime: PlaytimeCounts;
  played: PlayedCounts;
  lastPlayed: LastPlayedCounts;
  release: ReleaseCount[];
  added: AddedCounts;
  favoritesOnly: number;
  runningOnly: number;
  /** How many hidden rows Show hidden would reveal under the active facets. */
  showHidden: number;
}

export interface ScopeCounts {
  all: number;
  installed: number;
  favorites: number;
  recent: number;
  running: number;
}

export interface RowGroup {
  /** Stable group id, also used in `layout.collapsedGroups`. */
  id: string;
  /** Translation key the header renders; store groups carry their display name. */
  labelKey: string;
  /** The full group count, even while collapsed. */
  count: number;
  collapsed: boolean;
  /** Index of the group's first row in `LibraryView.rows`. */
  firstIndex: number;
}

export type TitleSourceKind = 'scope' | 'store' | 'collection' | 'savedView';

export interface TitleSource {
  kind: TitleSourceKind;
  id: string;
}

/** Everything a saved view restores. Search text is never saved. */
export interface SavedViewPayload {
  scope: LibraryScope;
  filters: LibraryFilters;
  sort: Sort;
  group: GroupBy;
  view: ViewMode;
}

/** One named view as the mock host stores it; the engine keeps it in SQLite per account. */
export interface SavedView {
  /** Zero until the store's AUTOINCREMENT answer supplies the real id. */
  id: number;
  name: string;
  payload: SavedViewPayload;
  position: number;
  createdAt: number;
}

/** Why the engine refused a view name; the dialog validates the same rules live. */
export type ViewError = 'empty' | 'duplicate';

/** Which saved-view write an attempt belongs to. */
export type ViewWriteOperation = 'save' | 'rename';

/** How a saved-view attempt ended. */
export type ViewWriteState = 'pending' | 'succeeded' | 'failed';

/** Why a saved-view attempt failed: a refused name, or the storage write itself. */
export type ViewWriteError = ViewError | 'failed';

/**
 * The latest saved-view attempt. `run` is a monotonically increasing attempt id (the
 * `PersistView` op once a write starts). Baseline it before submitting and react only to a
 * larger run: a failed write ends the spinner and can be retried, and a repeated validation
 * error still produces a new run instead of looking unchanged.
 */
export interface ViewWriteView {
  run: number;
  operation: ViewWriteOperation;
  /** The view being renamed; 0 for a new view. */
  viewId: number;
  /** The trimmed name the attempt used. */
  name: string;
  state: ViewWriteState;
  /** Set exactly when `state` is `'failed'`. */
  error: ViewWriteError | null;
  /** The database id after a successful save; null while pending or after a failure. */
  savedViewId: number | null;
}

/** One saved view as the sidebar lists it. */
export interface SavedViewCount {
  id: number;
  name: string;
  /** Rows the view's own payload would show now, over the hidden-filtered rows. */
  count: number;
  active: boolean;
}

/** One GameNode collection with the number of local rows in it. */
export interface CollectionCount {
  id: string;
  name: string;
  count: number;
}

/** One successful view save, for the "view saved" notification. */
export interface SavedViewNotice {
  id: number;
  name: string;
  /** Identifies the save so the UI notifies once per save. */
  run: number;
}

/** One hide run, for the notification and its Show again action. */
export interface HiddenNotice {
  keys: string[];
  hidden: boolean;
  /** Identifies the run so the UI notifies once per hide. */
  run: number;
}

export function createDefaultFilterCounts(): FilterCounts {
  return {
    stores: [],
    availability: { installed: 0, owned: 0, shared: 0 },
    matchStates: { matched: 0, ambiguous: 0, unmatched: 0, noGame: 0 },
    gamenodeStatus: {
      playing: 0,
      finished: 0,
      planned: 0,
      ongoing: 0,
      dropped: 0,
      inLibrary: 0,
      notInLibrary: 0,
      available: false,
    },
    playtime: { any: 0, atLeast10h: 0, atLeast100h: 0 },
    played: { any: 0, played: 0, unplayed: 0 },
    lastPlayed: {
      any: 0,
      last7Days: 0,
      last30Days: 0,
      last90Days: 0,
      lastYear: 0,
      olderThanYear: 0,
    },
    release: [],
    added: {
      any: 0,
      last7Days: 0,
      last30Days: 0,
      last90Days: 0,
      lastYear: 0,
      olderThanYear: 0,
    },
    favoritesOnly: 0,
    runningOnly: 0,
    showHidden: 0,
  };
}

export function createDefaultScopeCounts(): ScopeCounts {
  return { all: 0, installed: 0, favorites: 0, recent: 0, running: 0 };
}

export function createDefaultTitleSource(): TitleSource {
  return { kind: 'scope', id: 'all' };
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
  | 'targetNeedsApproval'
  | 'targetUnavailable'
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

export type AddMode = 'library' | 'catalog';

/**
 * What the add dialog was opened for. A catalog game has no local library key, so it carries the
 * GameNode game id instead.
 */
export type AddTarget = { row: string } | { game: number };

export interface AddTargetView {
  target: AddTarget;
  /** The local library key, for a row target. */
  entryKey: string | null;
  title: string;
  matched: boolean;
}

export interface CollectionOption {
  id: string;
  name: string;
  defaultStatus?: EntryStatus | null;
  picked: boolean;
}

export type ApiError = 'rejected' | 'failed';

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
  target: AddTarget;
  /** The local library key, for a row target. */
  entryKey: string | null;
  title: string;
  outcome: AddOutcome;
  /** The status GameNode has (or applied) for the game, when it is known. */
  status: EntryStatus | null;
}

export interface AddSummary {
  /** Identifies this run for the notification and for undoAdd. */
  runId: number;
  /** At least one entry was created and its id read back, so Undo can delete it. */
  undoable: boolean;
  items: AddItem[];
  added: number;
  alreadyPresent: number;
  skipped: number;
  failed: number;
}

export interface AddDialogView {
  targets: AddTargetView[];
  mode: AddMode;
  editingExisting: boolean;
  status: EntryStatus;
  appliedStatus: EntryStatus;
  appliedFrom: string | null;
  collections: CollectionsView;
  progress: AddProgress | null;
  error: ApiError | null;
  canConfirm: boolean;
  blocked: Reason | null;
  summary: AddSummary | null;
}

export type UndoState = 'done' | 'partial' | 'failed';

/** The result of one Undo add, for the notification. `run` is the add run it undid. */
export interface UndoAddView {
  run: number;
  state: UndoState;
  deleted: number;
  failed: number;
}

/** One finished batch status change, for the notification. */
export interface StatusRunView {
  run: number;
  status: EntryStatus;
  changed: number;
  skipped: number;
  failed: number;
  /** Keys with no GameNode game. */
  skippedUnmatched: number;
  /** Matched games the account library does not contain. */
  skippedNotInLibrary: number;
  /** Games with a status this build does not know; their server value is kept. */
  skippedUnknownStatus: number;
}

/** One finished collection add, for the notification. */
export interface CollectionRunView {
  run: number;
  collectionId: string;
  collectionName: string;
  changed: number;
  skipped: number;
  failed: number;
  skippedUnmatched: number;
  skippedNotInLibrary: number;
  skippedUnknownStatus: number;
  skippedAlreadyInCollection: number;
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
  /** Faceted option counts for the filter popover. */
  filterCounts: FilterCounts;
  /** Sidebar scope counts. */
  scopeCounts: ScopeCounts;
  /** Group headers, in their fixed order. Collapsed groups contribute no `rows`. */
  groups: RowGroup[];
  /** Saved views in sidebar order with their live counts and active highlight. */
  savedViews: SavedViewCount[];
  /** The account's GameNode collections with their local row counts. */
  collections: CollectionCount[];
  /** Why the last view name was refused, until a valid attempt. */
  viewError: ViewError | null;
  /** The latest saved-view attempt (save or rename), its run and its outcome. */
  viewWrite: ViewWriteView | null;
  /** The last successful save, for the notification. */
  lastSavedView: SavedViewNotice | null;
  /** The last hide run, for the notification and its Show again action. */
  lastHidden: HiddenNotice | null;
  /** False for the default library: there is nothing extra to save as a view. */
  canSaveView: boolean;
  titleSource: TitleSource;
  /** The in-app GameNode catalog search view. */
  catalog: CatalogView;
  /** Per-provider scan progress plus the DRM-free folders pseudo row. */
  scanProgress: ScanProgress[];
  /** Every non-hidden row, independent of filters, for the command palette. */
  paletteIndex: PaletteEntry[];
  scanning: boolean;
  failedProviders: string[];
  selected: string[];
  focused: string | null;
  actions: Actions;
  detail: DetailView | null;
  empty: EmptyState | null;
  matchDialog: MatchDialogView | null;
  addDialog: AddDialogView | null;
  /** The latest finished add, kept after its dialog closes for the notification and its Undo. */
  addSummary: AddSummary | null;
  /** The latest undo result, for the notification. */
  undo: UndoAddView | null;
  /** The latest finished batch status change, for the notification. */
  statusRun: StatusRunView | null;
  /** The latest finished collection add, for the notification. */
  collectionRun: CollectionRunView | null;
}

export type LaunchKind = 'play' | 'install';

export type LaunchError =
  | 'noHandler'
  | 'unsupported'
  | 'incomplete'
  | 'recipeNeedsAttention'
  | 'failed';

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
  /** Unix seconds of the last completed watched-folder check, kept across restarts. */
  discoveryLastCheckedAt: number | null;
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
  pluginRequest?: { pluginId: string; publisherFingerprint: string; remove: boolean } | null;
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
  /** Both strings are accepted for one release while hosts are updated. */
  | 'invalidCredential'
  | 'invalidCookies'
  | 'sharedUnavailable'
  /** The pre-rename name is accepted for one release, like the invalid-cookie pair above. */
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
  sharedCount: number;
  /** The provider's declared shared-access label; counts and warnings render only with one. */
  sharedLabel: string | null;
  installedCount: number;
  lastCheckedAt: number | null;
  /** Last successful catalog sync in this run; the connected toast waits for it. */
  lastSyncedAt?: number | null;
  storage: StorageMethod | null;
  persistFailed: boolean;
  catalogError: CatalogError | null;
  sharedError: CatalogError | null;
  canSignIn: boolean;
  canCancelSignIn: boolean;
  canFocusSignIn: boolean;
  canSignOut: boolean;
  canCheck: boolean;
  canRemoveCredential: boolean;
}

export interface PluginUpdatePreferences {
  periodicChecks: boolean;
}

export type PlaytimeAccountingMode = 'disabled' | 'application_open';

export interface PlaytimeCapabilities {
  identifiesGameplay: boolean;
  identifiesPauses: boolean;
  withoutPauseDetection: PlaytimeAccountingMode;
}

export interface PluginPlaytimeInfo {
  pluginId: string;
  name: string;
  version: string;
  playtime: PlaytimeCapabilities;
  accountingEnabled: boolean;
}

export interface AvailablePluginUpdate {
  pluginId: string;
  name: string;
  installedVersion: string;
  versions: string[];
}

export interface AvailablePluginRollback {
  pluginId: string;
  name: string;
  activeVersion: string;
  versions: string[];
}

export interface PluginUpdateReview {
  pluginId: string;
  installedVersion: string;
  candidateVersion: string;
  addedPermissions: string[];
  removedPermissions: string[];
  changedPermissions: string[];
  permissionChanges: Array<{
    id: string;
    installedScope: string;
    candidateScope: string;
    installedRequired: boolean;
    candidateRequired: boolean;
  }>;
  addedNativeAccess: string[];
  removedNativeAccess: string[];
  apiChanged: boolean;
  installedApi: string;
  candidateApi: string;
  licenseChanged: boolean;
  installedLicense: string;
  candidateLicense: string;
  installedLicenseFiles: string[];
  candidateLicenseFiles: string[];
  installedLicenseNotices: string[];
  candidateLicenseNotices: string[];
  installedPlaytime: PlaytimeCapabilities;
  candidatePlaytime: PlaytimeCapabilities;
  playtimeChanged: boolean;
  publisherChanged: boolean;
  installedPublisherKeyId: string;
  candidatePublisherKeyId: string;
  publisherFingerprint: string;
  changedDataSchemas: string[];
  preservesPriorData: boolean;
  isDowngrade: boolean;
  requiresManualApproval: boolean;
}

export interface PluginInfo {
  providerId: string;
  pluginId: string;
  pluginKind: 'provider' | 'extension';
  name: string;
  source: 'official' | 'community';
  status: 'installed' | 'missing' | 'unavailable' | 'failed' | 'incompatible';
  capabilities: string[];
  session: 'active' | 'stopped';
  nativeAccess: string[];
  version?: string;
  enabled?: boolean;
  runtimeAvailable?: boolean;
  /** The pinned publisher of a third-party plugin; absent for the bundled ones. */
  publisherFingerprint?: string;
}

export interface PluginImportReview {
  token: string;
  pluginId: string;
  providerId?: string;
  name: string;
  version: string;
  publisherKeyId: string;
  publisherFingerprint: string;
  pinnedFingerprint?: string;
  trust: 'trusted' | 'untrusted' | 'changed';
  license: string;
  licenseFiles: string[];
  licenseNotices: string[];
  permissionIds: string[];
  requestedPermissions: Array<{
    id: string;
    required: boolean;
    scope: string;
    reason: string;
  }>;
  nativeAccess: string[];
  compatibility: 'compatible' | 'incompatible' | 'unknown';
  compatibilityReason?: string;
}

/** Host-owned effective permission state for the currently active account profile. */
export interface PluginPermissionSnapshot {
  pluginId: string;
  publisherFingerprint: string;
  profileId: string;
  grantRevision: number;
  requestedPermissions: Array<{
    permission_id: string;
    required: boolean;
    allowed_scopes: PluginGrantScope[];
  }>;
  /** Rust grant records retain their serialized snake_case protocol fields. */
  effectiveGrants: Array<{
    permission_id: string;
    required: boolean;
    scope: Record<string, unknown>;
    state: 'granted' | 'denied' | 'unsupported';
  }>;
}

export type PluginGrantScope =
  | { scope: 'own-creator'; category: 'runtime' | 'library-root' | 'save-profile' }
  | { scope: 'public-category'; category: 'runtime' | 'library-root' | 'save-profile' }
  | { scope: 'selected-records'; category: 'runtime' | 'library-root' | 'save-profile'; record_ids: string[] }
  | { scope: 'declared'; value: string }
  | { scope: 'own-private-namespace' };

export interface PluginPermissionDecision {
  permission_id: string;
  scope: PluginGrantScope;
  granted: boolean;
}

/** Host-owned configuration availability; no plugin preference values are included. */
export interface PluginConfigurationState {
  pluginId: string;
  available: boolean;
  revision: string | null;
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
  | { markEntryNoGame: { key: string; now: number } }
  | 'clearMatch'
  | { select: { key: string; mode: SelectMode } }
  | { move: { direction: Direction; extend: boolean } }
  | { setColumns: number }
  | { setView: ViewMode }
  | { setFilter: SidebarSelection }
  | { setScope: LibraryScope }
  | { setGroup: GroupBy }
  | { toggleGroup: { id: string } }
  | { setAllGroupsCollapsed: boolean }
  | { setSearch: string }
  | { setSort: Sort }
  | { setFilters: LibraryFilters }
  | { saveView: { name: string } }
  | { renameView: { id: number; name: string } }
  | { deleteView: { id: number } }
  | { applyView: { id: number } }
  | { setHidden: { keys: string[]; hidden: boolean } }
  | 'selectAll'
  | 'clearSelection'
  | { setFavorite: { key: string; favorite: boolean; now: number } }
  | { setFavorites: { keys: string[]; favorite: boolean; now: number } }
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
  | { undoAdd: { run: number } }
  | { setStatus: { keys: string[]; status: EntryStatus } }
  | { addToCollection: { keys: string[]; collectionId: string } }
  | { useSuggestion: { key: string; gameId: number; now: number } }
  | { catalogSearch: string }
  | 'catalogClear'
  | { catalogFocus: number | null }
  | { openAddForGame: { gameId: number; title: string; coverUrl?: string | null } }
  | { removeEntry: { key: string; allowRediscovery: boolean } }
  | 'resetExcluded';

/** A browser page on the GameNode website. */
export type GameNodePage = 'library' | { game: { id: number } };

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
 * The host returns the canonical `gamenode-asset://localhost/achievement/<hash>` route. Rewrite
 * its base to the form this webview loads, like `coverAssetUrl` and `artworkAssetUrl` do, because
 * WebView2 on Windows cannot load the bare custom scheme.
 */
export function achievementIconAssetUrl(url: string | null | undefined): string | null {
  if (!url) return null;
  const marker = '/achievement/';
  const at = url.indexOf(marker);
  if (at < 0) return null;
  const hash = url.slice(at + marker.length).split(/[?#]/, 1)[0];
  if (!hash) return null;
  return `${assetProtocolBase()}${marker}${hash}`;
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

/** The idle catalog search the engine and the mock start from. */
export function createDefaultCatalogView(): CatalogView {
  return {
    query: '',
    status: 'idle',
    results: [],
    incomplete: false,
  };
}

/**
 * A complete, empty library view. The engine always serializes every field in
 * `LibraryView`, so fixtures and the mock spread this factory instead of leaving optional
 * placeholders. Every call returns fresh objects and arrays.
 */
export function createDefaultLibraryView(): LibraryView {
  return {
    generation: 0,
    layout: createDefaultLibraryLayout(),
    search: '',
    access: 'unverified',
    total: 0,
    installedTotal: 0,
    providers: [],
    rows: [],
    filterCounts: createDefaultFilterCounts(),
    scopeCounts: createDefaultScopeCounts(),
    groups: [],
    savedViews: [],
    collections: [],
    viewError: null,
    viewWrite: null,
    lastSavedView: null,
    lastHidden: null,
    canSaveView: false,
    titleSource: createDefaultTitleSource(),
    catalog: createDefaultCatalogView(),
    scanProgress: [],
    paletteIndex: [],
    scanning: false,
    failedProviders: [],
    selected: [],
    focused: null,
    actions: {
      selected: 0,
      play: { disabled: 'nothingSelected' },
      install: { disabled: 'nothingSelected' },
      matchGame: { disabled: 'nothingSelected' },
      addToGamenode: { disabled: 'nothingSelected' },
      changeMatch: false,
    },
    detail: null,
    empty: null,
    matchDialog: null,
    addDialog: null,
    addSummary: null,
    undo: null,
    statusRun: null,
    collectionRun: null,
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
      discoveryLastCheckedAt: null,
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
      sessionStartedAt: {},
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
