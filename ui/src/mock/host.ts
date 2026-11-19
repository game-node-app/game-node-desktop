/** An isolated, in-memory host for interactive UI development. */
import {
  createDefaultFilters,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultSnapshot,
  filtersAreDefault,
  getMatchedGameId,
  type AddSummary,
  type AuthInput,
  type CatalogStatus,
  type CollectionRunView,
  type LibraryInput,
  type LibraryLayout,
  type LibraryScope,
  type LibraryView,
  type EntryStatus,
  type PaletteEntry,
  type PluginCompatibilityDetails,
  type PluginInfo,
  type PluginUpdateReview,
  type Row,
  type SavedView,
  type SavedViewNotice,
  type SavedViewPayload,
  type HiddenNotice,
  type ScanProgress,
  type Snapshot,
  type StatusRunView,
  type StorageMethod,
  type StoreAccountState,
  type StoreAccountView,
  type SuggestionView,
  type UndoAddView,
  type ViewError,
  type ViewWriteView,
} from '../api/types';
import { groupId, providerCounts, runLibraryQuery, type ProviderRef } from './query';
import type {
  PortCatalogStatus,
  PortDetailView,
  PortEntryView,
  PortInstallRequest,
  PortJobPhase,
  PortJobView,
  PortPickSuppliedRequest,
  PortPlatform,
  PortPreferencesPatch,
  PortProgress,
  PortReleaseView,
  PortSetupSlot,
  PortSuppliedPick,
  PortToolsCacheView,
  PortsView,
} from '../api/ports';
import type { PluginNotice } from '../api/pluginNotices';
import type { PluginConfirmation, RememberedConfirmation } from '../api/pluginConfirmations';
import type { PluginFolderApproval, RootConfirmationPrompt, RootSelectionPrompt } from '../api/pluginFolderApprovals';

type Listener = (snapshot: Snapshot) => void;
type ProgressListener = (progress: PortProgress) => void;
type PluginNoticeListener = (notice: PluginNotice) => void;
type PluginConfirmationListener = (confirmation: unknown) => void;
type PluginConfirmationClosedListener = (id: string) => void;
type PluginRootPromptListener = (prompt: unknown) => void;
type PluginRootPromptClosedListener = (id: string) => void;

export type MockSteamScenario =
  | 'notConnected'
  | 'checking'
  | 'signingIn'
  | 'connected'
  | 'expired'
  | 'unreachable'
  | 'credentialError'
  | 'persistFailed'
  /** Connected, but the last catalog sync failed with an expired credential. */
  | 'catalogExpired'
  /** Connected, but the last catalog sync could not reach the store. */
  | 'catalogUnreachable';

const titles = ['Aster Vale', 'Beyond the Pines', 'Clockwork Harbor', 'Distant Skies', 'Emberfall', 'Forgotten Atlas', 'Glass Garden', 'Hollow Mountain', 'Infinite Coast', 'Juniper Station', 'Kingdom of Rain', 'Lunar Circuit'];

/**
 * The mock library mixes installed, owned-only and shared rows so every
 * indicator and count can be exercised from `VITE_UI_MOCK=1`.
 */
const NOW = Math.floor(Date.now() / 1000);
/** One stable successful-sync time per fixture run; banner dismissal identity relies on it. */
const LAST_SYNCED_AT = NOW - 120;

/** The account library fixture's collections; the sidebar lists these as collection scopes. */
export const MOCK_COLLECTIONS = [
  { id: 'backlog-curto', name: 'Backlog curto' },
  { id: 'coop-no-sofa', name: 'Coop no sofá' },
];

/** A hidden fixture entry, so Show hidden and the hidden counts can be exercised. */
export const MOCK_HIDDEN_KEY = 'steam:900';

/** The fixture saved view; a new host starts from it, and the mock keeps later changes in memory. */
const MOCK_SAVED_VIEWS: SavedView[] = [
  {
    id: 1,
    name: 'Steam 10 h+',
    payload: {
      scope: 'all',
      filters: {
        ...createDefaultFilters(),
        stores: ['steam'],
        playtime: 'atLeast10h',
      },
      sort: { key: 'playtime', direction: 'descending' },
      group: 'store',
      view: 'grid',
    },
    position: 0,
    createdAt: NOW - 3 * 86_400,
  },
];

/** Every GameNode status, plus an in-library entry without one and two games not in the library. */
const mockGamenodeStatuses: Array<EntryStatus | null> = [
  'playing', 'finished', 'planned', 'ongoing', 'dropped', null,
  'playing', 'finished', 'planned', 'ongoing', 'dropped', null,
];

function mockGamenode(index: number): Row['gamenode'] {
  // Unmatched rows have no account entry, so they count as Not on GameNode.
  if (index === 2 || index === 7) return null;
  const collectionIds =
    index % 4 === 0
      ? ['backlog-curto']
      : index % 4 === 1
        ? ['coop-no-sofa']
        : index % 4 === 3
          ? ['backlog-curto', 'coop-no-sofa']
          : [];
  return {
    status: mockGamenodeStatuses[index] ?? null,
    collectionIds,
    entryId: `entry-${index + 1}`,
  };
}

const allRows: Row[] = titles.map((title, index) => {
  const steam = index % 2 === 0;
  // GOG keeps the two favorites; the remaining odd rows stand in for Epic Games Store.
  const gog = index === 1 || index === 3;
  const provider = steam ? 'steam' : gog ? 'gog' : 'epic';
  const providerName = steam ? 'Steam' : gog ? 'GOG' : 'Epic Games Store';
  const installed = steam ? index === 0 || index === 2 || index === 8 : true;
  // Only Steam declares shared access in this fixture; an Epic row must stay owned.
  const shared = index === 6;
  const owned = !gog && !shared;
  return {
    key: `${provider}:${index + 1}`,
    title, detectedName: title, matchedName: index === 2 ? null : title,
    provider, providerName,
    installed, owned, shared,
    running: index === 0, status: index === 2 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
    sessionStartedAt: index === 0 ? NOW - 2530 : null,
    coverFile: `mock-${index}.svg`,
    playtime: {
      seconds: index * 3720,
      source: 'store',
      lastPlayedAt: index === 0 ? null : NOW - index * 86_400,
    },
    releaseYear: index === 2 ? null : 1992 + index * 2,
    addedAt: NOW - index * 3 * 86_400,
    isFavorite: index === 1 || index === 3,
    hidden: false,
    gamenode: mockGamenode(index),
  };
});
// A hidden fixture entry: it is scanned, matched and counted like any other row but stays out of
// the normal view, groups and Select all until Show hidden is on.
allRows.push({
  key: MOCK_HIDDEN_KEY,
  title: 'Hidden Harbor',
  detectedName: 'Hidden Harbor',
  matchedName: 'Hidden Harbor',
  provider: 'steam',
  providerName: 'Steam',
  installed: false,
  owned: true,
  shared: false,
  running: false,
  status: { matched: { gameId: 900, source: 'automatic' } },
  coverFile: 'mock-4.svg',
  playtime: { seconds: null, source: 'store', lastPlayedAt: null },
  releaseYear: 2015,
  addedAt: NOW - 20 * 86_400,
  isFavorite: false,
  hidden: true,
  gamenode: {
    status: 'planned',
    collectionIds: ['coop-no-sofa'],
    entryId: 'entry-900',
  },
});

const DAY = 86_400;
const MOCK_ACHIEVEMENTS = [
  { id: 'ACH_FIRST', name: 'First steps', description: 'Finish the opening chapter.', hidden: false, unlocked: true, unlockedAt: NOW - 300, position: 0 },
  { id: 'ACH_COLLECT', name: 'Collector', description: 'Find every hidden relic.', hidden: false, unlocked: true, unlockedAt: NOW - 7_200, position: 1 },
  { id: 'ACH_HIDDEN', name: 'A secret', description: 'A spoiler description.', hidden: true, unlocked: false, unlockedAt: null, position: 2 },
  { id: 'ACH_BOSS', name: 'Boss rush', description: 'Defeat every boss.', hidden: false, unlocked: false, unlockedAt: null, position: 3 },
  { id: 'ACH_NIGHT', name: 'Night owl', description: 'Play after midnight.', hidden: false, unlocked: false, unlockedAt: null, position: 4 },
];

function mockAchievementList(filter: string) {
  const records = MOCK_ACHIEVEMENTS
    .map((achievement) => ({
      ...achievement,
      iconUrl: '',
      iconGrayUrl: '',
    }))
    .filter((achievement) =>
      filter === 'unlocked'
        ? achievement.unlocked
        : filter === 'locked'
          ? !achievement.unlocked
          : true,
    );
  return {
    connected: true,
    state: 'ready' as const,
    lastSuccessAt: NOW - 60,
    failure: null,
    achievements: records,
  };
}

const MOCK_PROVIDERS: ProviderRef[] = [
  { id: 'steam', name: 'Steam' },
  { id: 'gog', name: 'GOG' },
  { id: 'epic', name: 'Epic Games Store' },
];

/**
 * Forty fictional catalog games for the mock catalog view (`Explorar no GameNode`). The first
 * twelve share their titles with the local fixture, so results can show both `In your library`
 * and `On this PC` without a network.
 */
export const MOCK_CATALOG_GAMES = [
  ...titles,
  'Amber Circuit',
  'Brass Meridian',
  'Cinder Orchard',
  'Drifting Ember',
  'Echo Foundry',
  'Fallow Skies',
  'Gilded Tundra',
  'Harborlight',
  'Ironwood Vale',
  'Jade Observatory',
  'Kestrel Run',
  'Lumen Tides',
  'Moss & Machine',
  'Nightjar',
  'Obsidian Choir',
  'Pale Horizon',
  'Quartz Harbor',
  'River of Static',
  'Saltwind',
  'Tidal Archive',
  'Umbra Station',
  'Velvet Aurora',
  'Wandering Signal',
  'Xenon Garden',
  'Yellow Static',
  'Zenith Drift',
  'Atlas of Ash',
  'Copper Moon',
].map((title, index) => ({
  gameId: index + 1,
  title,
  year: 2004 + ((index * 3) % 20),
  coverUrl: `/mock-covers/mock-${index % 12}.svg`,
}));

/** Suggested games for the fixture's unmatched DRM-free-looking entry. */
const MOCK_SUGGESTIONS: Record<string, SuggestionView[]> = {
  'steam:3': [
    {
      gameId: 201,
      title: 'Clockwork Harbor',
      year: 2016,
      coverUrl: '/mock-covers/mock-2.svg',
      confidence: 95,
      tone: 'success',
    },
    {
      gameId: 202,
      title: 'Clockwork Harbor: Wound Spring',
      year: 2021,
      coverUrl: '/mock-covers/mock-8.svg',
      confidence: 62,
      tone: 'warning',
    },
  ],
  // L6 ambiguous fixture: the two candidates the server could not separate.
  'steam:1301': [
    {
      gameId: 301,
      title: 'Neon Courier 2088',
      year: 2023,
      coverUrl: '/mock-covers/mock-3.svg',
      confidence: 91,
      tone: 'success',
    },
    {
      gameId: 302,
      title: 'Neon Courier',
      year: 2019,
      coverUrl: '/mock-covers/mock-9.svg',
      confidence: 58,
      tone: 'warning',
    },
  ],
  // L6 unmatched DRM-free fixture: a plausible but not perfect title match.
  'manual:drift': [
    {
      gameId: 303,
      title: 'Drifting Ember',
      year: 2022,
      coverUrl: '/mock-covers/mock-5.svg',
      confidence: 84,
      tone: 'success',
    },
    {
      gameId: 304,
      title: 'Drifter: Ember Trail',
      year: 2018,
      coverUrl: '/mock-covers/mock-1.svg',
      confidence: 47,
      tone: 'neutral',
    },
  ],
};

function mockPlaytimeLabel(seconds: number | null | undefined): string | null {
  if (seconds == null || seconds < 0) return null;
  const hours = Math.floor(seconds / 3600);
  if (hours > 0) return `${hours} h`;
  return `${Math.floor((seconds % 3600) / 60)} min`;
}

/** Every non-hidden row, independent of filters, for the command palette. */
function mockPaletteIndex(source: Row[]): PaletteEntry[] {
  return source
    .filter((row) => !row.hidden)
    .map((row) => ({
      key: row.key,
      title: row.title,
      providerName: row.providerName,
      installed: row.installed,
      playtimeLabel: mockPlaytimeLabel(row.playtime.seconds),
    }));
}

/** Per-store progress followed by the DRM-free folders pseudo row. */
function mockScanProgress(source: Row[], discoveryScanning: boolean): ScanProgress[] {
  return [
    ...MOCK_PROVIDERS.map((provider) => ({
      provider: provider.id,
      name: provider.name,
      state: 'done' as const,
      found: source.filter((row) => row.provider === provider.id).length,
    })),
    {
      provider: 'drm-free',
      name: 'DRM-free folders',
      state: discoveryScanning ? ('scanning' as const) : ('done' as const),
      found: 2,
    },
  ];
}

function createLibrary(
  source: Row[],
  initialScope: LibraryScope = 'all',
  providers: ProviderRef[] = MOCK_PROVIDERS,
): LibraryView {
  const layout = {
    ...createDefaultLibraryLayout(),
    scope: initialScope,
    detailOpen: false,
  };
  const query = runLibraryQuery({
    rows: source,
    layout,
    search: '',
    now: NOW,
    providers,
    gamenodeAvailable: true,
  });
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    layout,
    search: '',
    access: 'verified',
    total: query.total,
    installedTotal: query.installedTotal,
    providers: providerCounts(source, query.filterCounts.stores, layout.filters.showHidden),
    rows: structuredClone(query.rows),
    filterCounts: structuredClone(query.filterCounts),
    scopeCounts: query.scopeCounts,
    groups: query.groups,
    titleSource: query.titleSource,
    catalog: { query: '', status: 'idle', results: [], incomplete: false },
    scanProgress: mockScanProgress(source, false),
    paletteIndex: mockPaletteIndex(source),
    scanning: false,
    failedProviders: [],
    selected: [],
    focused: null,
    actions: { selected: 0, play: { disabled: 'nothingSelected' }, install: { disabled: 'nothingSelected' }, matchGame: { disabled: 'nothingSelected' }, addToGamenode: { disabled: 'nothingSelected' }, changeMatch: false },
  };
}

/**
 * Optional large-library fixture (`?library=large`): 320 owned Steam games, exactly 18 installed,
 * and a few shared games, so virtualization, counts and the Installed filter can be checked
 * against a realistic catalog. It is additive; the default scenario stays at 12 games.
 */
function createLargeLibraryRows(): Row[] {
  const rows: Row[] = [];
  for (let index = 0; index < 320; index += 1) {
    const installed = index < 18;
    rows.push({
      key: `steam:${1000 + index}`,
      title: `Steam Saga ${index + 1}`,
      detectedName: `Steam Saga ${index + 1}`,
      matchedName: index % 5 === 0 ? null : `Steam Saga ${index + 1}`,
      provider: 'steam',
      providerName: 'Steam',
      installed,
      owned: true,
      shared: false,
      running: false,
      status: index % 5 === 0 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
      coverFile: `mock-${index % 12}.svg`,
      playtime: { seconds: installed ? index * 600 : null, source: 'store', lastPlayedAt: installed ? NOW - index * 3600 : null },
      releaseYear: 2000 + (index % 25),
      addedAt: NOW - index * DAY,
      isFavorite: false,
      hidden: false,
      gamenode: null,
    });
  }
  for (let index = 0; index < 8; index += 1) {
    rows.push({
      key: `steam:${5000 + index}`,
      title: `Shared Shelf ${index + 1}`,
      detectedName: `Shared Shelf ${index + 1}`,
      matchedName: null,
      provider: 'steam',
      providerName: 'Steam',
      installed: false,
      owned: false,
      shared: true,
      running: false,
      status: 'unmatched',
      coverFile: `mock-${index % 12}.svg`,
      playtime: { seconds: null, source: 'store', lastPlayedAt: null },
      releaseYear: null,
      addedAt: NOW - index * DAY,
      isFavorite: false,
      hidden: false,
      gamenode: null,
    });
  }
  return rows;
}

/** How long the mock takes to answer a saved-view storage write, so `pending` is observable. */
const MOCK_VIEW_WRITE_DELAY = 140;

/**
 * Comprehensive opt-in library fixture (`?library=variants`). It keeps every default row and
 * adds the match shapes and DRM-free registration the L6 detail variants need:
 * ambiguous, marked-not-a-game, a running matched DRM-free game with saves, and an unmatched
 * DRM-free game with scored suggestions. The default fixture is untouched.
 */
function createVariantRows(): Row[] {
  const base = structuredClone(allRows).map((row) =>
    row.key === 'steam:1'
      ? { ...row, running: false, sessionStartedAt: null }
      : row,
  );
  return [
    ...base,
    {
      key: 'steam:1301',
      title: 'Neon Courier 2088',
      detectedName: 'neon_courier',
      matchedName: null,
      provider: 'steam',
      providerName: 'Steam',
      installed: true,
      owned: true,
      shared: false,
      running: false,
      status: { ambiguous: { candidates: [301, 302] } },
      coverFile: 'mock-3.svg',
      playtime: { seconds: 14 * 3600, source: 'store', lastPlayedAt: NOW - 2 * DAY },
      releaseYear: null,
      addedAt: NOW - 2 * DAY,
      isFavorite: false,
      hidden: false,
      gamenode: null,
    },
    {
      key: 'steam:1302',
      title: 'Wallpaper Foundry',
      detectedName: 'wallpaper_foundry',
      matchedName: null,
      provider: 'steam',
      providerName: 'Steam',
      installed: true,
      owned: true,
      shared: false,
      running: false,
      status: 'noGame',
      coverFile: 'mock-6.svg',
      playtime: { seconds: 0, source: 'store', lastPlayedAt: null },
      releaseYear: null,
      addedAt: NOW - 9 * DAY,
      isFavorite: false,
      hidden: false,
      gamenode: null,
    },
    {
      key: 'manual:brasa',
      title: 'Brasa',
      detectedName: 'Brasa',
      matchedName: 'Brasa',
      provider: 'manual',
      providerName: 'DRM-free',
      installed: true,
      owned: true,
      shared: false,
      running: true,
      sessionStartedAt: NOW - 2530,
      status: { matched: { gameId: 305, source: 'chosen' } },
      coverFile: 'mock-7.svg',
      playtime: { seconds: 3 * 3600 + 5 * 60, source: 'internal', lastPlayedAt: NOW - DAY },
      releaseYear: 2024,
      addedAt: NOW - 30 * DAY,
      isFavorite: false,
      hidden: false,
      gamenode: {
        status: 'ongoing',
        collectionIds: ['backlog-curto'],
        entryId: 'entry-brasa',
      },
    },
    {
      key: 'manual:drift',
      title: 'Drift',
      detectedName: 'Drift',
      matchedName: null,
      provider: 'manual',
      providerName: 'DRM-free',
      installed: true,
      owned: true,
      shared: false,
      running: false,
      status: 'unmatched',
      coverFile: 'mock-5.svg',
      playtime: { seconds: 0, source: 'internal', lastPlayedAt: null },
      releaseYear: null,
      addedAt: NOW - 5 * DAY,
      isFavorite: false,
      hidden: false,
      gamenode: null,
    },
  ];
}

/** The DRM-free registrations, profile, backups and profile history behind the variant rows. */
function applyVariantLocalGames(local: Snapshot['localGames']) {
  local.watches = [
    { id: 'watched-games', path: 'D:\\Jogos', enabled: true },
  ];
  local.registrations = [
    {
      id: 'brasa',
      title: 'Brasa',
      executable: 'D:\\Jogos\\Brasa\\brasa.exe',
      fileIdentity: 'mock-brasa-exe',
      root: 'D:\\Jogos\\Brasa',
      workingDirectory: 'D:\\Jogos\\Brasa',
      arguments: ['--windowed', '--fps', '120'],
      gameplay: [],
      launchers: [],
      revision: 1,
      removed: false,
      available: true,
    },
    {
      id: 'drift',
      title: 'Drift',
      executable: 'D:\\Jogos\\Drift\\drift.exe',
      fileIdentity: 'mock-drift-exe',
      root: 'D:\\Jogos\\Drift',
      workingDirectory: 'D:\\Jogos\\Drift',
      arguments: [],
      gameplay: [],
      launchers: [],
      revision: 1,
      removed: false,
      available: true,
    },
  ];
  local.profiles = [
    {
      id: 'brasa-profile',
      installation: 'brasa',
      revision: 1,
      title: 'Brasa',
      manifestHash: 'mock-brasa-manifest',
      definition: 'mock-brasa-definition',
      files: ['%APPDATA%\\Brasa'],
      registry: [],
      support: 'supported',
      automatic: true,
    },
  ];
  local.versions = Array.from({ length: 12 }, (_, index) => ({
    installation: 'brasa',
    profile: 'brasa-profile',
    destination: 'D:\\Backups\\Brasa',
    id: `brasa-backup-${index + 1}`,
    label: `Backup ${index + 1}`,
    bytes: 24_000_000 + index * 1024,
  }));
  local.jobs = [
    {
      id: 'brasa-job-2',
      installation: 'brasa',
      profile: 'brasa-profile',
      revision: 1,
      playPeriod: 'session',
      destination: 'D:\\Backups\\Brasa',
      state: 'successful',
      attempts: 1,
      readyAt: NOW - 3_600,
      backupId: 'brasa-backup-12',
      error: null,
    },
    {
      id: 'brasa-job-1',
      installation: 'brasa',
      profile: 'brasa-profile',
      revision: 1,
      playPeriod: 'session',
      destination: 'D:\\Backups\\Brasa',
      state: 'successful',
      attempts: 1,
      readyAt: NOW - 2 * DAY,
      backupId: 'brasa-backup-11',
      error: null,
    },
  ];
  local.settings = { automatic: true, destination: 'D:\\Backups' };
  local.discoveryCompleted = true;
  local.discoveryLastCheckedAt = NOW - 12 * 60;
}

/** Three watched-folder candidates for the discovered banner and its review (`?discoveries=1`). */
function applyDiscoveryFixture(local: Snapshot['localGames']) {
  const candidates: Snapshot['localGames']['candidates'] = [
    {
      fingerprint: 'candidate-signal-ridge',
      watch: 'watched-games',
      root: 'D:\\Jogos\\Signal Ridge',
      title: 'Signal Ridge',
      targets: ['D:\\Jogos\\Signal Ridge\\signal.exe'],
      selectedTarget: 'D:\\Jogos\\Signal Ridge\\signal.exe',
      evidence: [{ kind: 'executable', value: 'signal.exe', strong: true }],
      ignored: false,
      incomplete: false,
    },
    {
      fingerprint: 'candidate-paper-harbor',
      watch: 'watched-games',
      root: 'D:\\Jogos\\Paper Harbor',
      title: 'Paper Harbor',
      targets: ['D:\\Jogos\\Paper Harbor\\paper.exe'],
      selectedTarget: 'D:\\Jogos\\Paper Harbor\\paper.exe',
      evidence: [{ kind: 'executable', value: 'paper.exe', strong: true }],
      ignored: false,
      incomplete: false,
    },
    {
      fingerprint: 'candidate-tiny-comet',
      watch: 'watched-games',
      root: 'D:\\Jogos\\Tiny Comet',
      title: 'Tiny Comet',
      targets: [
        'D:\\Jogos\\Tiny Comet\\comet.exe',
        'D:\\Jogos\\Tiny Comet\\comet64.exe',
      ],
      selectedTarget: 'D:\\Jogos\\Tiny Comet\\comet.exe',
      evidence: [{ kind: 'executable', value: 'comet.exe', strong: true }],
      ignored: false,
      incomplete: false,
    },
  ];
  local.candidates = candidates;
  local.recentlyAdded = candidates.map((candidate) => candidate.title);
  local.reviewSelected = [];
  local.watches = local.watches.length > 0
    ? local.watches
    : [{ id: 'watched-games', path: 'D:\\Jogos', enabled: true }];
  local.discoveryCompleted = true;
}

function installedOnlyRows(source: Row[], provider: string): Row[] {  return source
    .map((row) => row.provider === provider ? { ...row, owned: false, shared: false } : { ...row })
    .filter((row) => row.installed || row.owned || row.shared);
}

function makeStoreAccount(
  provider: 'steam' | 'epic',
  scenario: MockSteamScenario,
  counts: Pick<StoreAccountView, 'ownedCount' | 'sharedCount' | 'installedCount'>,
  storage: StorageMethod,
): StoreAccountView {
  const disconnected = scenario === 'notConnected';
  const signingIn = scenario === 'signingIn';
  const catalogExpired = scenario === 'catalogExpired';
  const catalogUnreachable = scenario === 'catalogUnreachable';
  const catalogScenario = catalogExpired || catalogUnreachable;
  const state: StoreAccountState =
    scenario === 'persistFailed' || catalogScenario ? 'connected' : scenario;
  const identity = scenario === 'notConnected' || signingIn
    ? null
    : provider === 'steam'
      ? { accountId: '76561198000000000', accountName: 'cassi' }
      : { accountId: 'epic-account-1', accountName: 'EpicPlayer' };
  const catalogError =
    scenario === 'expired' || catalogExpired
      ? 'expired'
      : scenario === 'unreachable' || catalogUnreachable
        ? 'unreachable'
        : null;

  return {
    provider,
    state,
    accountId: identity?.accountId ?? null,
    accountName: identity?.accountName ?? null,
    ownedCount: disconnected ? 0 : counts.ownedCount,
    sharedCount: disconnected || provider === 'epic' ? 0 : counts.sharedCount,
    sharedLabel: provider === 'steam' ? 'Steam Family' : null,
    installedCount: counts.installedCount,
    lastCheckedAt: disconnected || signingIn ? null : Math.floor(Date.now() / 1000) - 120,
    lastSyncedAt: disconnected || signingIn ? null : LAST_SYNCED_AT,
    storage,
    persistFailed: scenario === 'persistFailed',
    catalogError,
    sharedError: null,
    canSignIn:
      scenario === 'notConnected' ||
      scenario === 'expired' ||
      scenario === 'credentialError' ||
      catalogExpired,
    canCancelSignIn: signingIn,
    canFocusSignIn: signingIn,
    canSignOut:
      state === 'connected' ||
      scenario === 'expired' ||
      scenario === 'unreachable' ||
      scenario === 'persistFailed' ||
      catalogScenario,
    canCheck:
      state === 'connected' ||
      scenario === 'unreachable' ||
      scenario === 'credentialError' ||
      scenario === 'persistFailed' ||
      catalogScenario,
    canRemoveCredential: scenario === 'credentialError',
  };
}

// ---------------------------------------------------------------------------------------
// PC ports fixture
// ---------------------------------------------------------------------------------------

/** Asset size used by the simulated install progress, in bytes. */
const MOCK_PORT_ASSET_BYTES = 1_820_000_000;

const RUNNING_PORT_PHASES: PortJobPhase[] = [
  'queued',
  'resolving',
  'downloading',
  'verifying',
  'extracting',
  'suppliedFiles',
  'finalizing',
  'registering',
  'cancelling',
];

function mockSlot(slot: PortSetupSlot): PortSetupSlot {
  return slot;
}

/**
 * Fictional Windows-compatible entries covering every catalog state the UI must render:
 * a guided recipe with a console-package option, a plain release, a pre-release only, a
 * withdrawn entry, an installed entry with an update, an installer-only entry and one
 * entry whose install deliberately fails so the failure details can be exercised.
 */
function createMockPortEntries(): PortEntryView[] {
  const packageOption = {
    extractorId: 'ps4-pkg',
    toolName: 'Mock PKG Extractor',
    toolSourceUrl: 'https://example.invalid/tools/pkg-extractor',
    toolLicense: 'GPL-3.0',
    titleId: 'CUSA10101',
    updateVersion: '01.04',
    requiresUpdate: true,
  };

  return [
    {
      catalogId: 'qcat_mock_aster',
      slug: 'aster-vale-revival',
      name: 'Aster Vale: Revival',
      games: ['Aster Vale'],
      consoles: ['PlayStation 4'],
      supportedOS: ['windows'],
      verified: true,
      prerelease: false,
      version: '1.4.2',
      guided: true,
      withdrawn: false,
      hasWindowsAsset: true,
      noAssetReason: null,
      installable: true,
      installBlockReason: null,
      art: { capsule: true, hero: true },
      project: {
        label: 'Aster Vale Revival',
        url: 'https://example.invalid/ports/aster-vale-revival',
      },
      source: {
        label: 'Aster Vale (PlayStation 4)',
        url: 'https://example.invalid/games/aster-vale',
      },
      license: 'GPL-3.0-or-later',
      requirements: [
        mockSlot({
          id: 'game-folder',
          label: 'Aster Vale (CUSA10101) game folder',
          prompt: 'Choose the folder from your own disc with update 01.04 applied.',
          kind: 'folder',
          required: true,
          extensions: [],
          contains: ['eboot.bin', 'sce_sys/param.sfo'],
          acceptedLabels: ['CUSA10101 (01.04)'],
          destination: 'CUSA10101',
          root: 'game',
          argument: null,
          packageOption,
        }),
        mockSlot({
          id: 'settings-backup',
          label: 'Settings backup (optional)',
          prompt: 'An exported settings file gives the port better defaults.',
          kind: 'file',
          required: false,
          extensions: ['.zip'],
          contains: [],
          acceptedLabels: [],
          destination: null,
          root: null,
          argument: { kind: 'flag', name: '--settings' },
          packageOption: null,
        }),
      ],
      installedRegistrationKey: null,
    },
    {
      catalogId: 'qcat_mock_beyond',
      slug: 'beyond-the-pines',
      name: "Beyond the Pines: Director's Cut",
      games: ['Beyond the Pines'],
      consoles: ['PlayStation 3'],
      supportedOS: ['windows'],
      verified: true,
      prerelease: false,
      version: '0.9.1',
      guided: false,
      withdrawn: false,
      hasWindowsAsset: true,
      noAssetReason: null,
      installable: true,
      installBlockReason: null,
      art: { capsule: true, hero: false },
      project: {
        label: 'Beyond the Pines port',
        url: 'https://example.invalid/ports/beyond-the-pines',
      },
      source: null,
      license: null,
      requirements: [],
      installedRegistrationKey: null,
    },
    {
      catalogId: 'qcat_mock_clockwork',
      slug: 'clockwork-harbor',
      name: 'Clockwork Harbor',
      games: ['Clockwork Harbor'],
      consoles: ['PlayStation 4', 'PC'],
      supportedOS: ['windows'],
      verified: false,
      prerelease: true,
      version: '2.0.0-alpha.3',
      guided: false,
      withdrawn: false,
      hasWindowsAsset: true,
      noAssetReason: null,
      installable: false,
      installBlockReason: 'noVerifiedRelease',
      art: { capsule: false, hero: true },
      project: {
        label: 'Clockwork Harbor nightly builds',
        url: 'https://example.invalid/ports/clockwork-harbor',
      },
      source: null,
      license: 'MIT',
      requirements: [],
      installedRegistrationKey: null,
    },
    {
      catalogId: 'qcat_mock_distant',
      slug: 'distant-skies',
      name: 'Distant Skies: Recompiled',
      games: ['Distant Skies'],
      consoles: ['PlayStation 3'],
      supportedOS: ['windows'],
      verified: true,
      prerelease: false,
      version: '1.1.0',
      guided: false,
      withdrawn: true,
      hasWindowsAsset: true,
      noAssetReason: null,
      installable: false,
      installBlockReason: 'withdrawn',
      art: { capsule: false, hero: false },
      project: {
        label: 'Distant Skies recompilation',
        url: 'https://example.invalid/ports/distant-skies',
      },
      source: null,
      license: null,
      requirements: [],
      installedRegistrationKey: null,
    },
    {
      catalogId: 'qcat_mock_emberfall',
      slug: 'emberfall-iron-trail',
      name: 'Emberfall: Iron Trail',
      games: ['Emberfall'],
      consoles: ['PlayStation 4'],
      supportedOS: ['windows'],
      verified: true,
      prerelease: false,
      version: '1.0.5',
      guided: false,
      withdrawn: false,
      hasWindowsAsset: true,
      noAssetReason: null,
      installable: true,
      installBlockReason: null,
      art: { capsule: true, hero: true },
      project: {
        label: 'Emberfall Iron Trail',
        url: 'https://example.invalid/ports/emberfall-iron-trail',
      },
      source: {
        label: 'Emberfall (PlayStation 4)',
        url: 'https://example.invalid/games/emberfall',
      },
      license: 'GPL-3.0-only',
      requirements: [],
      installedRegistrationKey: 'drmfree:mock-emberfall',
    },
    {
      catalogId: 'qcat_mock_glass',
      slug: 'glass-garden-enhanced',
      name: 'Glass Garden: Enhanced',
      games: ['Glass Garden'],
      consoles: ['PC'],
      supportedOS: ['windows'],
      verified: true,
      prerelease: false,
      version: '3.0.0',
      guided: false,
      withdrawn: false,
      hasWindowsAsset: false,
      noAssetReason: 'installerOnly',
      installable: false,
      installBlockReason: 'installerOnly',
      art: { capsule: true, hero: false },
      project: {
        label: 'Glass Garden Enhanced',
        url: 'https://example.invalid/ports/glass-garden-enhanced',
      },
      source: null,
      license: null,
      requirements: [],
      installedRegistrationKey: null,
    },
    {
      catalogId: 'qcat_mock_hollow',
      slug: 'hollow-mountain-deep-delve',
      name: 'Hollow Mountain: Deep Delve',
      games: ['Hollow Mountain'],
      consoles: ['PlayStation 4'],
      supportedOS: ['windows'],
      verified: true,
      prerelease: false,
      version: '0.4.0',
      guided: true,
      withdrawn: false,
      hasWindowsAsset: true,
      noAssetReason: null,
      installable: true,
      installBlockReason: null,
      art: { capsule: false, hero: false },
      project: {
        label: 'Hollow Mountain Deep Delve',
        url: 'https://example.invalid/ports/hollow-mountain',
      },
      source: null,
      license: null,
      requirements: [
        mockSlot({
          id: 'game-package',
          label: 'Hollow Mountain (CUSA20202) packages',
          prompt: 'Base package and the 01.01 update package.',
          kind: 'folder',
          required: true,
          extensions: [],
          contains: ['eboot.bin'],
          acceptedLabels: ['CUSA20202 (01.01)'],
          destination: null,
          root: null,
          argument: null,
          packageOption: {
            ...packageOption,
            titleId: 'CUSA20202',
            updateVersion: '01.01',
          },
        }),
      ],
      installedRegistrationKey: null,
    },
  ];
}

function mockRelease(entry: PortEntryView, version: string): PortReleaseView {
  return {
    version,
    releasedAt: '2026-09-18',
    verified: entry.verified,
    withdrawn: entry.withdrawn,
    assetName: entry.hasWindowsAsset ? `${entry.slug}-windows.zip` : null,
    assetFormat: entry.hasWindowsAsset ? 'zip' : null,
    sizeBytes: entry.hasWindowsAsset ? MOCK_PORT_ASSET_BYTES : null,
    checksumVerified: entry.verified,
    requiresChecksumConfirmation: !entry.verified && entry.hasWindowsAsset,
    malwareFlag: null,
  };
}

function mockDetailFor(entry: PortEntryView): PortDetailView {
  const release = entry.version ? mockRelease(entry, entry.version) : null;
  const installed = entry.installedRegistrationKey
    ? entry.catalogId === 'qcat_mock_emberfall'
    : false;
  return {
    catalogId: entry.catalogId,
    description:
      'A native PC port built by the community from the original console release. GameNode installs it from your own files and never supplies game content.',
    developer: 'Mock Port Works',
    license: entry.license,
    project: entry.project,
    source: entry.source,
    art: entry.art,
    requirements: entry.requirements,
    releases: release ? [release] : [],
    selectedRelease: entry.installable ? release : null,
    withdrawalReason: entry.withdrawn
      ? 'The author withdrew this release upstream.'
      : null,
    updateAvailable: installed && release ? release : null,
    noAssetReason: entry.noAssetReason,
  };
}

function createMockPortsView(scenario: 'default' | 'unavailable'): PortsView {
  const entries = createMockPortEntries();
  const unavailable = scenario === 'unavailable';
  const catalog: PortCatalogStatus = unavailable
    ? { state: 'unavailable', fetchedAt: null, entryCount: 0, error: 'unreachable' }
    : { state: 'stale', fetchedAt: NOW - 2 * DAY, entryCount: entries.length, error: null };
  const failedJob: PortJobView = {
    jobId: 'job-mock-failed',
    catalogId: 'qcat_mock_beyond',
    slug: 'beyond-the-pines',
    name: "Beyond the Pines: Director's Cut",
    phase: 'failed',
    slotId: null,
    outcome: 'failed',
    failure: {
      message: 'The downloaded archive could not be verified.',
      toolOutput: null,
    },
    interruptible: false,
    retryable: true,
    seen: false,
    startedAt: NOW - 90,
    finishedAt: NOW - 60,
  };
  return {
    catalog,
    entries: unavailable ? [] : entries,
    provenance: {
      'drmfree:mock-emberfall': {
        registrationKey: 'drmfree:mock-emberfall',
        catalogId: 'qcat_mock_emberfall',
        slug: 'emberfall-iron-trail',
        portName: 'Emberfall: Iron Trail',
        installedVersion: '1.0.4',
        installFolder: 'D:\\Ports\\Emberfall',
        jobId: 'job-mock-previous',
        installedAt: NOW - 5 * DAY,
      },
    },
    jobs: unavailable ? [] : [failedJob],
    unseenCount: unavailable ? 0 : 1,
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

export function createMockHost(options: {
  signedOut?: boolean;
  /** Signed-in first-run: no library, no connected store and no watched folder. */
  firstRun?: boolean;
  locale?: string;
  storeAccount?: MockSteamScenario;
  steamStorage?: StorageMethod;
  largeLibrary?: boolean;
  /** Comprehensive L6 fixture: every match state plus registered DRM-free rows. */
  variantLibrary?: boolean;
  /** Signed-in first-run fixture with no library rows. */
  emptyLibrary?: boolean;
  /** Owned and family-shared rows that are all not installed; starts on Installed. */
  noInstalledLibrary?: boolean;
  /** Watched-folder candidates for the discovered banner and review. */
  discoveries?: boolean;
  /** Starts a rescan on load, for the scanning panel and skeleton states. */
  startScanning?: boolean;
  /** How a saved-view storage write fails: never, once, or every time. */
  viewWriteFailure?: 'none' | 'once' | 'always';
  platform?: PortPlatform;
  portsScenario?: 'default' | 'unavailable';
  catalogScenario?: 'default' | 'error' | 'offline' | 'incomplete';
  failMutations?: boolean;
} = {}) {
  const listeners = new Set<Listener>();
  const progressListeners = new Set<ProgressListener>();
  const pluginNoticeListeners = new Set<PluginNoticeListener>();
  const confirmationListeners = new Set<PluginConfirmationListener>();
  const confirmationClosedListeners = new Set<PluginConfirmationClosedListener>();
  const rootSelectionListeners = new Set<PluginRootPromptListener>();
  const rootConfirmationListeners = new Set<PluginRootPromptListener>();
  const rootPromptClosedListeners = new Set<PluginRootPromptClosedListener>();
  /** Confirmations the fictional user left open, and what they chose to remember, by plugin. */
  const openConfirmations = new Map<string, PluginConfirmation>();
  const openRootSelections = new Map<string, RootSelectionPrompt>();
  const openRootConfirmations = new Map<string, RootConfirmationPrompt>();
  const remembered = new Map<string, RememberedConfirmation[]>();
  const mockPublisher = 'cd'.repeat(32);
  const folderApprovals = new Map<string, PluginFolderApproval[]>([[
    'community.example.save-sync',
    [
      {
        id: 'mock-file-approval', pluginId: 'community.example.save-sync', publisherFingerprint: mockPublisher,
        installationName: 'Aster Vale', kind: 'save', root: 'C:\\Games\\Aster Vale\\Saves',
        operations: ['read'], affectedGames: ['Aster Vale'], revoked: false, declaredByPlugin: false,
        logicalLibrary: false, libraryId: null, watching: null,
      },
      {
        id: 'mock-library-root', pluginId: 'community.example.save-sync', publisherFingerprint: mockPublisher,
        installationName: 'Community library', kind: 'library', root: 'C:\\Games',
        operations: ['read', 'plugin_discovery', 'core_executable_discovery'], affectedGames: [],
        revoked: false, declaredByPlugin: true, logicalLibrary: true, libraryId: 'mock-library', watching: true,
      },
    ],
  ]]);
  let periodicPluginChecks = false;
  const mockUpdatePlugin = 'community.example.save-sync';
  let mockPluginVersion = '1.0.0';
  let mockRollbackVersion: string | undefined;
  let mockUpdateReview: PluginUpdateReview | undefined;
  const mockCompatibility: PluginCompatibilityDetails = {
    state: 'compatible', ambiguous: false, os: 'ubuntu', release: '24.04', architecture: 'x86_64', abi: 'gnu',
    libc: { family: 'gnu', version: '2.39' }, emulation: 'native',
    targets: [{
      target: {
        os: 'linux', arch: 'x86_64', abi: 'gnu', entrypoint: 'bin/linux-x86_64-gnu/save-sync',
        requires: { linux: { libc: { family: 'glibc', min_version: '2.31' } } },
        tested_on: [{ distribution: 'ubuntu', version: '22.04' }],
      },
      verdict: { state: 'compatible', reasons: ['requirementsSatisfied'], unknownFacts: [] },
    }],
  };
  const mockUpdates = () => mockPluginVersion === '1.0.0' ? [{
    pluginId: mockUpdatePlugin, name: 'Community Save Sync', installedVersion: mockPluginVersion, versions: ['2.0.0'],
  }] : [];
  const snapshot = createDefaultSnapshot();
  // The status bar fixture: the watched folders were checked 12 minutes ago.
  snapshot.localGames.discoveryLastCheckedAt = NOW - 12 * 60;
  if (options.variantLibrary) applyVariantLocalGames(snapshot.localGames);
  if (options.discoveries) applyDiscoveryFixture(snapshot.localGames);
  if (options.firstRun) {
    snapshot.localGames.watches = [];
    snapshot.localGames.registrations = [];
    snapshot.localGames.profiles = [];
    snapshot.localGames.versions = [];
    snapshot.localGames.jobs = [];
    snapshot.localGames.candidates = [];
    snapshot.localGames.recentlyAdded = [];
    snapshot.localGames.reviewSelected = [];
    snapshot.localGames.discoveryCompleted = false;
    snapshot.localGames.discoveryLastCheckedAt = null;
  }
  const ports = createMockPortsView(options.portsScenario ?? 'default');
  snapshot.ports = ports;
  const platform = options.platform ?? 'windows';
  const epicEnabled = platform !== 'linux';
  // The manual provider only exists in the comprehensive fixture; the default store list stays
  // at the three stores so its counts and sidebar are unchanged.
  const queryProviders: ProviderRef[] = options.variantLibrary
    ? [...MOCK_PROVIDERS, { id: 'manual', name: 'DRM-free' }]
    : MOCK_PROVIDERS;
  const baseRows = options.firstRun || options.emptyLibrary
    ? []
    : options.largeLibrary
      ? createLargeLibraryRows()
      : options.variantLibrary
        ? createVariantRows()
        : options.noInstalledLibrary
          ? allRows.map((row) => ({ ...row, installed: false }))
          : allRows;
  const initialScope: LibraryScope = options.noInstalledLibrary ? 'installed' : 'all';
  let rows = structuredClone(baseRows);
  let gridColumns = 1;
  /** The selection the engine keeps while the filters, search or grouping mask it. */
  let selectedKeys = new Set<string>();
  let focusedKey: string | null = null;
  /** Bumped on an account change so an older view write can never land in a new session. */
  let accountGeneration = 0;

  /** Mirrors the host's tracker: one running fixture with its real process start. */
  function syncTracking() {
    const running = rows.find((row) => row.running);
    const startedAt = running?.sessionStartedAt;
    snapshot.tracking = {
      title: running?.title ?? '',
      available: true,
      running: running ? [running.key] : [],
      sessionStartedAt:
        running && startedAt != null ? { [running.key]: startedAt } : {},
    };
  }
  syncTracking();

  // The account library fixture: every local game with a GameNode entry, plus one account-only
  // catalog game so membership can be shown without a local row.
  const accountLibraryGameIds = new Set<number>(
    rows
      .filter((row) => row.gamenode)
      .map((row) => getMatchedGameId(row.status))
      .filter((gameId): gameId is number => gameId != null),
  );
  accountLibraryGameIds.add(13);

  // The catalog view's raw state. Results keep only catalog metadata; membership and the local
  // key are recomputed on every update, so an add or undo flips them without another search.
  let catalogFocus: number | null = null;
  let catalogQuery = '';
  let catalogStatus: CatalogStatus = 'idle';
  let catalogResults: typeof MOCK_CATALOG_GAMES = [];
  let catalogIncomplete = false;
  let catalogOperation = 0;
  let scanProgressOverride: ScanProgress[] | null = null;
  const catalogMetadata = new Map<
    number,
    { gameId: number; title: string; year: number | null; coverUrl: string | null }
  >(MOCK_CATALOG_GAMES.map((game) => [game.gameId, game]));

  /** The account's saved views and hidden keys; a new host starts from the fixtures. */
  let savedViews: SavedView[] = options.firstRun || options.emptyLibrary
    ? []
    : structuredClone(MOCK_SAVED_VIEWS);
  const hiddenKeys = new Set<string>(
    options.firstRun || options.emptyLibrary ? [] : [MOCK_HIDDEN_KEY],
  );
  let appliedViewPayload: SavedViewPayload | null = null;
  let appliedViewId: number | null = null;
  let viewError: ViewError | null = null;
  let viewWrite: ViewWriteView | null = null;
  let viewWriteFailuresLeft = options.viewWriteFailure === 'once' ? 1 : 0;
  const pendingViewWrites = new Map<
    number,
    {
      timer: ReturnType<typeof setTimeout>;
      operation: ViewWriteView['operation'];
      viewId: number;
    }
  >();
  let lastSavedView: SavedViewNotice | null = null;
  let lastHidden: HiddenNotice | null = null;
  let organizationRun = 0;
  const collections = options.firstRun || options.emptyLibrary ? [] : MOCK_COLLECTIONS;
  // The batch GameNode writes. The failure toggle stands in for a failed request.
  let batchRun = 0;
  let lastAdd: AddSummary | null = null;
  let lastUndo: UndoAddView | null = null;
  let lastStatusRun: StatusRunView | null = null;
  let lastCollectionRun: CollectionRunView | null = null;
  let createdByLastAdd: Array<{ gameId: number; entryId: string }> = [];

  type StoreId = 'steam' | 'epic';
  const storeIds: StoreId[] = epicEnabled ? ['steam', 'epic'] : ['steam'];
  const scenarios: Record<StoreId, MockSteamScenario> = {
    steam: options.firstRun ? 'notConnected' : options.storeAccount ?? 'connected',
    epic: options.firstRun ? 'notConnected' : 'connected',
  };
  const previousScenarios: Record<StoreId, MockSteamScenario> = {
    steam: scenarios.steam === 'signingIn' ? 'notConnected' : scenarios.steam,
    epic: scenarios.epic === 'signingIn' ? 'notConnected' : scenarios.epic,
  };
  const storage: Record<StoreId, StorageMethod> = {
    steam: options.steamStorage ?? 'secretService',
    epic: 'secretService',
  };
  const generations: Record<StoreId, number> = { steam: 0, epic: 0 };

  function providerCountsOf(provider: StoreId) {
    return {
      ownedCount: rows.filter((row) => row.provider === provider && row.owned).length,
      sharedCount: rows.filter((row) => row.provider === provider && row.shared && !row.owned).length,
      installedCount: rows.filter((row) => row.provider === provider && row.installed).length,
    };
  }

  function syncStoreAccounts() {
    snapshot.storeAccounts = storeIds.map((provider) =>
      makeStoreAccount(provider, scenarios[provider], providerCountsOf(provider), storage[provider]),
    );
  }

  function rowsForScenarios(): Row[] {
    let next: Row[] = structuredClone(baseRows).map((row) => ({
      ...row,
      hidden: hiddenKeys.has(row.key),
    }));
    if (!epicEnabled) {
      next = next.filter((row) => row.provider !== 'epic');
    }
    for (const provider of storeIds) {
      if (scenarios[provider] === 'notConnected') {
        next = installedOnlyRows(next, provider);
      }
    }
    return next;
  }

  /** Drops selected or focused entries a provider catalog change removed, as the engine does. */
  function pruneSelectionToRows() {
    const present = new Set(rows.map((row) => row.key));
    for (const key of selectedKeys) {
      if (!present.has(key)) selectedKeys.delete(key);
    }
    if (focusedKey != null && !present.has(focusedKey)) focusedKey = null;
  }

  function clearCatalog(provider: StoreId) {
    rows = rows
      .map((row) => row.provider === provider ? { ...row, owned: false, shared: false } : row)
      .filter((row) => row.installed || row.owned || row.shared);
    pruneSelectionToRows();
  }

  function scheduleStore(provider: StoreId, delay: number, action: () => void) {
    const generation = ++generations[provider];
    setTimeout(() => {
      if (generation !== generations[provider]) return;
      action();
    }, delay);
  }

  /** The current catalog view, with membership and local keys read from live fixture state. */
  function catalogView(): NonNullable<LibraryView['catalog']> {
    const localKeys = new Map<number, string>();
    for (const row of rows) {
      const gameId = getMatchedGameId(row.status);
      if (gameId != null && !localKeys.has(gameId)) localKeys.set(gameId, row.key);
    }
    return {
      query: catalogQuery,
      status: catalogStatus,
      results: catalogResults.map((game) => ({
        gameId: game.gameId,
        title: game.title,
        year: game.year,
        coverUrl: game.coverUrl,
        coverFile: `mock-${(game.gameId - 1) % 12}.svg`,
        hasArtwork: catalogFocus === game.gameId,
        inLibrary:
          accountLibraryGameIds.has(game.gameId) ||
          rows.some(
            (row) =>
              getMatchedGameId(row.status) === game.gameId && row.gamenode != null,
          ),
        localKey: localKeys.get(game.gameId) ?? null,
      })),
      incomplete: catalogIncomplete,
    };
  }

  /** The mock search is asynchronous like the engine's: 250 ms, latest operation wins. */
  function catalogSearch(query: string) {
    catalogFocus = null;
    catalogQuery = query;
    const trimmed = query.trim();
    const operation = ++catalogOperation;
    if (trimmed.length < 2) {
      catalogStatus = 'idle';
      catalogResults = [];
      catalogIncomplete = false;
      return;
    }
    if (snapshot.auth === 'signedOut' || options.catalogScenario === 'offline') {
      catalogStatus = 'offline';
      catalogResults = [];
      catalogIncomplete = false;
      return;
    }
    catalogStatus = 'loading';
    catalogResults = [];
    catalogIncomplete = false;
    setTimeout(() => {
      if (operation !== catalogOperation) return;
      if (options.catalogScenario === 'error') {
        catalogStatus = 'error';
        catalogResults = [];
        catalogIncomplete = false;
        publish();
        return;
      }
      catalogStatus = 'ready';
      catalogResults = MOCK_CATALOG_GAMES.filter((game) =>
        game.title.toLowerCase().includes(trimmed.toLowerCase()),
      );
      catalogIncomplete = options.catalogScenario === 'incomplete';
      publish();
    }, 250);
  }

  function catalogClear() {
    catalogOperation += 1;
    catalogQuery = '';
    catalogStatus = 'idle';
    catalogResults = [];
    catalogIncomplete = false;
  }

  /** Opens the Add to GameNode dialog for one catalog game, without inventing a local row. */
  function openAddForGame(gameId: number, title: string, coverUrl?: string | null) {
    const lib = snapshot.library;
    if (!lib) return;
    const existing = catalogMetadata.get(gameId);
    const resolvedTitle = title.trim() || existing?.title || `Game ${gameId}`;
    const resolvedCover = coverUrl ?? existing?.coverUrl ?? null;
    catalogMetadata.set(gameId, {
      gameId,
      title: resolvedTitle,
      year: existing?.year ?? null,
      coverUrl: resolvedCover,
    });
    lib.addDialog = {
      targets: [
        {
          target: { game: gameId },
          entryKey: null,
          title: resolvedTitle,
          matched: true,
        },
      ],
      mode: 'catalog',
      editingExisting: false,
      status: 'playing',
      appliedStatus: 'playing',
      appliedFrom: null,
      collections: { loaded: [{ id: 'favorites', name: 'Favorites', picked: false }] },
      progress: null,
      error: null,
      canConfirm: true,
      blocked: null,
      summary: null,
    };
  }

  /** Suggestions for the selected unmatched entry, as the detail pane shows them. */
  function suggestionsFor(row: Row): SuggestionView[] {
    const ambiguous =
      row.status === 'ambiguous' ||
      (typeof row.status === 'object' && 'ambiguous' in row.status);
    if (row.status !== 'unmatched' && !ambiguous) return [];
    return MOCK_SUGGESTIONS[row.key] ?? [];
  }

  function useSuggestion(key: string, gameId: number) {
    const row = rows.find((item) => item.key === key);
    const suggestion = MOCK_SUGGESTIONS[key]?.find((item) => item.gameId === gameId);
    if (!row || !suggestion) return;
    row.status = { matched: { gameId, source: 'chosen' } };
    row.matchedName = suggestion.title;
    row.title = suggestion.title;
  }

  /** Rescan keeps the rows usable while the progress panel walks waiting → scanning → done. */
  function startMockScan() {
    const lib = snapshot.library;
    if (!lib) return;
    scanProgressOverride = [
      ...MOCK_PROVIDERS.map((provider) => ({
        provider: provider.id,
        name: provider.name,
        state: 'waiting' as const,
        found: null,
      })),
      {
        provider: 'drm-free',
        name: 'DRM-free folders',
        state: 'waiting' as const,
        found: null,
      },
    ];
    lib.scanning = true;
    snapshot.localGames.discoveryScanning = true;
    publish();
    setTimeout(() => {
      scanProgressOverride = [
        ...MOCK_PROVIDERS.map((provider) => ({
          provider: provider.id,
          name: provider.name,
          state: 'scanning' as const,
          found: null,
        })),
        {
          provider: 'drm-free',
          name: 'DRM-free folders',
          state: 'scanning' as const,
          found: null,
        },
      ];
      publish();
    }, 120);
    setTimeout(() => {
      scanProgressOverride = mockScanProgress(rows, false);
      snapshot.localGames.discoveryScanning = false;
      lib.scanning = false;
      publish();
    }, 700);
  }


  // ---- PC ports simulation ----
  const jobTimers = new Map<string, Set<ReturnType<typeof setTimeout>>>();
  let jobCounter = 0;
  let portRefreshGeneration = 0;

  // The Settings Tools panel's fictional PC ports cache. The extractor is the same fictional
  // tool the guided setup offers, and both values are non-zero so Clear cache is visible.
  let portsDownloadsBytes = 734_003_200;
  let portsHelperTools = [
    {
      id: 'ps4-pkg',
      name: 'Fixture PKG extractor',
      license: 'GPL-3.0',
      sourceUrl: 'https://github.com/example/fixture-pkg',
      version: 'v2.4.1',
      cacheBytes: 18_874_368,
    },
  ];

  function portInstallRunning(): boolean {
    return ports.jobs.some((job) => RUNNING_PORT_PHASES.includes(job.phase));
  }

  function mockPortToolsCache(): PortToolsCacheView {
    return {
      downloadsBytes: portsDownloadsBytes,
      helperTools: portsHelperTools.map((tool) => ({ ...tool })),
      installActive: portInstallRunning(),
    };
  }

  function clearMockPortCache(): PortToolsCacheView {
    if (portInstallRunning()) {
      // The host's typed refusal; the panel only needs to know the clear did not happen.
      throw { code: 'installInProgress' };
    }
    portsDownloadsBytes = 0;
    portsHelperTools = portsHelperTools.map((tool) => ({ ...tool, cacheBytes: 0 }));
    return mockPortToolsCache();
  }

  function nowSeconds(): number {
    return Math.floor(Date.now() / 1000);
  }

  function emitPortProgress(progress: PortProgress) {
    progressListeners.forEach((listener) => listener(progress));
  }

  function recomputeUnseenInstalls() {
    ports.unseenCount = ports.jobs.filter(
      (job) =>
        RUNNING_PORT_PHASES.includes(job.phase) ||
        (!job.seen && job.outcome !== 'cancelled'),
    ).length;
  }

  function findPortJob(jobId: string): PortJobView | undefined {
    return ports.jobs.find((job) => job.jobId === jobId);
  }

  function scheduleJobStep(jobId: string, delay: number, step: () => void) {
    const timer = setTimeout(() => {
      const timers = jobTimers.get(jobId);
      if (!timers) return;
      timers.delete(timer);
      step();
    }, delay);
    const timers = jobTimers.get(jobId) ?? new Set<ReturnType<typeof setTimeout>>();
    timers.add(timer);
    jobTimers.set(jobId, timers);
  }

  function setPortJobPhase(jobId: string, phase: PortJobPhase, slotId: string | null = null) {
    const job = findPortJob(jobId);
    if (!job) return;
    job.phase = phase;
    job.slotId = slotId;
    job.interruptible = RUNNING_PORT_PHASES.includes(phase) && phase !== 'cancelling';
    publish();
  }

  function refreshPortCatalog() {
    const generation = ++portRefreshGeneration;
    ports.catalog = { ...ports.catalog, state: 'refreshing', error: null };
    publish();
    setTimeout(() => {
      if (generation !== portRefreshGeneration) return;
      if (options.portsScenario === 'unavailable') {
        ports.catalog = {
          state: 'unavailable',
          fetchedAt: null,
          entryCount: 0,
          error: 'unreachable',
        };
      } else {
        ports.catalog = {
          state: 'ready',
          fetchedAt: nowSeconds(),
          entryCount: ports.entries.length,
          error: null,
        };
      }
      publish();
    }, 600);
  }

  function completePortJob(jobId: string, entry: PortEntryView) {
    const job = findPortJob(jobId);
    if (!job) return;
    const registrationKey = `drmfree:${entry.slug}`;
    job.phase = 'completed';
    job.outcome = 'completed';
    job.slotId = null;
    job.interruptible = false;
    job.retryable = false;
    job.finishedAt = nowSeconds();
    // A completed install leaves its verified download in the fictional cache.
    portsDownloadsBytes = MOCK_PORT_ASSET_BYTES;
    entry.installedRegistrationKey = registrationKey;
    ports.provenance[registrationKey] = {
      registrationKey,
      catalogId: entry.catalogId,
      slug: entry.slug,
      portName: entry.name,
      installedVersion: entry.version ?? job.name,
      installFolder: `D:\\Ports\\${entry.slug}`,
      jobId,
      installedAt: nowSeconds(),
    };
    recomputeUnseenInstalls();
    jobTimers.delete(jobId);
    publish();
  }

  function failPortJob(jobId: string, message: string, toolOutput: string | null) {
    const job = findPortJob(jobId);
    if (!job) return;
    job.phase = 'failed';
    job.outcome = 'failed';
    job.failure = { message, toolOutput };
    job.interruptible = false;
    job.retryable = true;
    job.finishedAt = nowSeconds();
    recomputeUnseenInstalls();
    jobTimers.delete(jobId);
    publish();
  }

  function cancelPortJob(jobId: string) {
    const job = findPortJob(jobId);
    if (!job || !job.interruptible) return;
    jobTimers.get(jobId)?.forEach((timer) => clearTimeout(timer));
    jobTimers.delete(jobId);
    job.phase = 'cancelling';
    job.interruptible = false;
    publish();
    setTimeout(() => {
      const current = findPortJob(jobId);
      if (!current) return;
      current.phase = 'cancelled';
      current.outcome = 'cancelled';
      current.retryable = true;
      current.finishedAt = nowSeconds();
      recomputeUnseenInstalls();
      publish();
    }, 300);
  }

  function startPortInstall(request: PortInstallRequest): string | null {
    const entry = ports.entries.find((item) => item.catalogId === request.catalogId);
    if (!entry || !entry.installable) return null;

    // The plan refuses a package choice that omits the update the recipe requires. The mock
    // answers with the same typed problem the real command returns.
    for (const slot of entry.requirements) {
      const choices = request.supplied.filter((choice) => choice.slotId === slot.id);
      const usesPackage = choices.some((choice) => choice.package);
      const hasUpdate = choices.some((choice) => choice.package && choice.update);
      if (slot.packageOption?.requiresUpdate && usesPackage && !hasUpdate) {
        throw {
          code: 'installBlocked',
          message: {
            code: 'missingRequiredUpdate',
            slot: slot.id,
            version: slot.packageOption.updateVersion ?? '?',
          },
        };
      }
    }

    const jobId = `job-mock-${++jobCounter}`;
    const job: PortJobView = {
      jobId,
      catalogId: entry.catalogId,
      slug: entry.slug,
      name: entry.name,
      phase: 'queued',
      slotId: null,
      outcome: null,
      failure: null,
      interruptible: true,
      retryable: false,
      seen: false,
      startedAt: nowSeconds(),
      finishedAt: null,
    };
    ports.jobs = [job, ...ports.jobs];
    ports.preferences.lastParentFolder = request.parentFolder;
    recomputeUnseenInstalls();
    jobTimers.set(jobId, new Set());
    publish();

    // A synchronous first tick lets the bridge test observe the event without waiting.
    emitPortProgress({
      jobId,
      phase: 'downloading',
      slotId: null,
      bytesDone: 0,
      bytesTotal: MOCK_PORT_ASSET_BYTES,
    });

    scheduleJobStep(jobId, 120, () => setPortJobPhase(jobId, 'resolving'));
    scheduleJobStep(jobId, 280, () => setPortJobPhase(jobId, 'downloading'));
    for (let tick = 1; tick <= 6; tick += 1) {
      scheduleJobStep(jobId, 280 + tick * 110, () => {
        const current = findPortJob(jobId);
        if (!current || current.phase !== 'downloading') return;
        emitPortProgress({
          jobId,
          phase: 'downloading',
          slotId: null,
          bytesDone: Math.round((MOCK_PORT_ASSET_BYTES * tick) / 6),
          bytesTotal: MOCK_PORT_ASSET_BYTES,
        });
      });
    }
    scheduleJobStep(jobId, 1000, () => setPortJobPhase(jobId, 'verifying'));
    scheduleJobStep(jobId, 1250, () => setPortJobPhase(jobId, 'extracting'));

    if (entry.catalogId === 'qcat_mock_hollow') {
      const slotId = entry.requirements[0]?.id ?? null;
      scheduleJobStep(jobId, 1500, () => setPortJobPhase(jobId, 'suppliedFiles', slotId));
      scheduleJobStep(jobId, 1750, () =>
        failPortJob(
          jobId,
          'The extraction tool could not read the package.',
          'pkg-extract: encrypted package (CUSA20202)',
        ),
      );
      return jobId;
    }

    if (entry.requirements.length > 0) {
      const slotId = entry.requirements[0]?.id ?? null;
      scheduleJobStep(jobId, 1500, () => {
        setPortJobPhase(jobId, 'suppliedFiles', slotId);
        emitPortProgress({
          jobId,
          phase: 'hashing',
          slotId,
          bytesDone: 0,
          bytesTotal: 4_000_000_000,
        });
      });
      scheduleJobStep(jobId, 1700, () =>
        emitPortProgress({
          jobId,
          phase: 'hashing',
          slotId,
          bytesDone: 2_000_000_000,
          bytesTotal: 4_000_000_000,
        }),
      );
      scheduleJobStep(jobId, 1900, () =>
        emitPortProgress({
          jobId,
          phase: 'copying',
          slotId,
          bytesDone: 4_000_000_000,
          bytesTotal: 4_000_000_000,
        }),
      );
    }

    scheduleJobStep(jobId, 2100, () => setPortJobPhase(jobId, 'finalizing'));
    scheduleJobStep(jobId, 2300, () => setPortJobPhase(jobId, 'registering'));
    scheduleJobStep(jobId, 2550, () => completePortJob(jobId, entry));
    return jobId;
  }

  function applyPortPreferences(patch: PortPreferencesPatch) {
    if (patch.view) ports.preferences.view = patch.view;
    if (patch.filters) {
      ports.preferences.filters = { ...ports.preferences.filters, ...patch.filters };
    }
    if ('lastParentFolder' in patch) {
      ports.preferences.lastParentFolder = patch.lastParentFolder ?? null;
    }
    publish();
  }

  function markPortsSeen(catalogId: string) {
    for (const job of ports.jobs) {
      if (job.catalogId === catalogId && !RUNNING_PORT_PHASES.includes(job.phase)) {
        job.seen = true;
      }
    }
    recomputeUnseenInstalls();
    publish();
  }

  // ---- Saved views and hidden entries (the engine's organization module) ----

  function savedViewId(scope: LibraryScope): number | null {
    return typeof scope === 'object' && 'savedView' in scope ? scope.savedView : null;
  }

  /** The effective scope: a highlighted view contributes its payload, never the marker. */
  function effectiveScopeOf(layout: LibraryLayout): LibraryScope {
    return savedViewId(layout.scope) != null && appliedViewPayload
      ? appliedViewPayload.scope
      : layout.scope;
  }

  function payloadKey(payload: SavedViewPayload): string {
    const filters = payload.filters;
    return JSON.stringify({
      scope: payload.scope,
      filters: {
        availability: [
          filters.availability.installed,
          filters.availability.owned,
          filters.availability.shared,
        ],
        stores: filters.stores,
        matchStates: filters.matchStates,
        gamenodeStatus: filters.gamenodeStatus,
        playtime: filters.playtime,
        played: filters.played,
        lastPlayed: filters.lastPlayed,
        release: filters.release,
        added: filters.added,
        favoritesOnly: filters.favoritesOnly,
        runningOnly: filters.runningOnly,
        showHidden: filters.showHidden,
      },
      sort: [payload.sort.key, payload.sort.direction],
      group: payload.group,
      view: payload.view,
    });
  }

  function samePayload(a: SavedViewPayload, b: SavedViewPayload): boolean {
    return payloadKey(a) === payloadKey(b);
  }

  function currentViewPayload(lib: LibraryView): SavedViewPayload {
    return {
      scope: effectiveScopeOf(lib.layout),
      filters: lib.layout.filters,
      sort: lib.layout.sort,
      group: lib.layout.group,
      view: lib.layout.view,
    };
  }

  function payloadIsDefault(payload: SavedViewPayload): boolean {
    return (
      payload.scope === 'all' &&
      filtersAreDefault(payload.filters) &&
      payload.sort.key === 'title' &&
      payload.sort.direction === 'ascending' &&
      payload.group === 'none' &&
      payload.view === 'grid'
    );
  }

  function viewRowCount(view: SavedView): number {
    const query = runLibraryQuery({
      rows,
      layout: {
        ...createDefaultLibraryLayout(),
        ...structuredClone(view.payload),
        collapsedGroups: {},
      },
      search: '',
      now: NOW,
      providers: queryProviders,
      gamenodeAvailable: true,
    });
    return query.visibleCount;
  }

  /** Mirrors the engine: leave the highlighted view when its payload changed. */
  function reconcileViewHighlight() {
    const lib = snapshot.library;
    if (!lib) {
      appliedViewPayload = null;
      appliedViewId = null;
      return;
    }
    const marker = savedViewId(lib.layout.scope);
    if (marker == null) {
      appliedViewPayload = null;
      appliedViewId = null;
      return;
    }
    if (marker !== appliedViewId || !appliedViewPayload) {
      appliedViewPayload = null;
      appliedViewId = null;
      return;
    }
    if (!samePayload(currentViewPayload(lib), appliedViewPayload)) {
      lib.layout.scope = appliedViewPayload.scope;
      appliedViewPayload = null;
      appliedViewId = null;
    }
  }

  function nextViewId(): number {
    return savedViews.reduce((max, view) => Math.max(max, view.id), 0) + 1;
  }

  /** Keeps the newest attempt visible; an older answer never replaces a newer run. */
  function recordViewWrite(write: ViewWriteView) {
    if (!viewWrite || write.run >= viewWrite.run) viewWrite = write;
  }

  function viewWriteShouldFail(): boolean {
    if (options.viewWriteFailure === 'always') return true;
    if (viewWriteFailuresLeft > 0) {
      viewWriteFailuresLeft -= 1;
      return true;
    }
    return false;
  }

  /**
   * Answers one storage write after a short delay, like the engine's storage worker. The
   * account generation guards a write that was in flight when the account changed.
   */
  function scheduleViewWrite(
    run: number,
    operation: ViewWriteView['operation'],
    viewId: number,
    name: string,
    onSuccess: () => number,
  ) {
    const generation = accountGeneration;
    const timer = setTimeout(() => {
      pendingViewWrites.delete(run);
      if (generation !== accountGeneration || !snapshot.library) return;
      if (viewWriteShouldFail()) {
        // The store kept its previous state; memory does too. No phantom view appears.
        recordViewWrite({
          run,
          operation,
          viewId,
          name,
          state: 'failed',
          error: 'failed',
          savedViewId: null,
        });
      } else {
        const savedViewId = onSuccess();
        viewError = null;
        recordViewWrite({
          run,
          operation,
          viewId,
          name,
          state: 'succeeded',
          error: null,
          savedViewId,
        });
      }
      publish();
    }, MOCK_VIEW_WRITE_DELAY);
    pendingViewWrites.set(run, { timer, operation, viewId });
  }

  function saveView(name: string) {
    const lib = snapshot.library;
    if (!lib) return;
    viewError = null;
    const payload = structuredClone(currentViewPayload(lib));
    // The UI only offers Save as view when this is true; a direct input is a no-op.
    if (payloadIsDefault(payload)) return;
    const run = ++organizationRun;
    const operation: ViewWriteView['operation'] = 'save';
    const trimmed = name.trim();
    if (trimmed.length === 0) {
      viewError = 'empty';
      recordViewWrite({
        run,
        operation,
        viewId: 0,
        name: trimmed,
        state: 'failed',
        error: 'empty',
        savedViewId: null,
      });
      return;
    }
    if (savedViews.some((view) => view.name.trim().toLowerCase() === trimmed.toLowerCase())) {
      viewError = 'duplicate';
      recordViewWrite({
        run,
        operation,
        viewId: 0,
        name: trimmed,
        state: 'failed',
        error: 'duplicate',
        savedViewId: null,
      });
      return;
    }
    recordViewWrite({
      run,
      operation,
      viewId: 0,
      name: trimmed,
      state: 'pending',
      error: null,
      savedViewId: null,
    });
    scheduleViewWrite(run, operation, 0, trimmed, () => {
      const view: SavedView = {
        id: nextViewId(),
        name: trimmed,
        payload,
        position: savedViews.length,
        createdAt: NOW,
      };
      savedViews = [...savedViews, view];
      lastSavedView = { id: view.id, name: view.name, run };
      return view.id;
    });
  }

  function renameView(id: number, name: string) {
    viewError = null;
    const view = savedViews.find((candidate) => candidate.id === id);
    if (!view) return;
    const run = ++organizationRun;
    const operation: ViewWriteView['operation'] = 'rename';
    const trimmed = name.trim();
    if (trimmed.length === 0) {
      viewError = 'empty';
      recordViewWrite({
        run,
        operation,
        viewId: id,
        name: trimmed,
        state: 'failed',
        error: 'empty',
        savedViewId: null,
      });
      return;
    }
    if (
      savedViews.some(
        (candidate) =>
          candidate.id !== id &&
          candidate.name.trim().toLowerCase() === trimmed.toLowerCase(),
      )
    ) {
      viewError = 'duplicate';
      recordViewWrite({
        run,
        operation,
        viewId: id,
        name: trimmed,
        state: 'failed',
        error: 'duplicate',
        savedViewId: null,
      });
      return;
    }
    recordViewWrite({
      run,
      operation,
      viewId: id,
      name: trimmed,
      state: 'pending',
      error: null,
      savedViewId: null,
    });
    scheduleViewWrite(run, operation, id, trimmed, () => {
      const target = savedViews.find((candidate) => candidate.id === id);
      if (target) target.name = trimmed;
      return id;
    });
  }

  function deleteView(id: number) {
    const lib = snapshot.library;
    const index = savedViews.findIndex((view) => view.id === id);
    if (index < 0) return;
    // A rename still in flight can never land once its view is gone; stop waiting.
    for (const [run, pending] of [...pendingViewWrites]) {
      if (pending.viewId !== id || pending.operation !== 'rename') continue;
      clearTimeout(pending.timer);
      pendingViewWrites.delete(run);
      if (viewWrite?.run === run && viewWrite.state === 'pending') {
        recordViewWrite({
          ...viewWrite,
          state: 'failed',
          error: 'failed',
          savedViewId: null,
        });
      }
    }
    savedViews = savedViews.filter((view) => view.id !== id);
    if (!lib || savedViewId(lib.layout.scope) !== id) return;
    // Deleting the active view keeps its effective filters, scope, sort, grouping and view mode.
    lib.layout.scope =
      appliedViewId === id && appliedViewPayload ? appliedViewPayload.scope : 'all';
    appliedViewPayload = null;
    appliedViewId = null;
  }

  function applyView(id: number) {
    const lib = snapshot.library;
    const view = savedViews.find((candidate) => candidate.id === id);
    if (!lib || !view) return;
    const payload = structuredClone(view.payload);
    appliedViewPayload = payload;
    appliedViewId = id;
    lib.layout.scope = { savedView: id };
    lib.layout.filters = payload.filters;
    lib.layout.sort = payload.sort;
    lib.layout.group = payload.group;
    lib.layout.view = payload.view;
    reconcileViewHighlight();
  }

  function setHidden(keys: string[], hidden: boolean) {
    const changed = [...new Set(keys)].filter((key) => hiddenKeys.has(key) !== hidden);
    if (changed.length === 0) return;
    for (const key of changed) {
      if (hidden) hiddenKeys.add(key);
      else hiddenKeys.delete(key);
    }
    rows = rows.map((row) =>
      changed.includes(row.key) ? { ...row, hidden } : row,
    );
    if (hidden) {
      organizationRun += 1;
      lastHidden = { keys: changed, hidden: true, run: organizationRun };
      for (const key of changed) selectedKeys.delete(key);
      if (focusedKey != null && changed.includes(focusedKey)) focusedKey = null;
    }
  }

  function nextBatchRun(): number {
    batchRun += 1;
    return batchRun;
  }

  function pickedCollections(dialog: NonNullable<LibraryView['addDialog']>): string[] {
    const collections = dialog.collections;
    if (typeof collections !== 'object' || !('loaded' in collections)) return [];
    return collections.loaded.filter((collection) => collection.picked).map((collection) => collection.id);
  }

  /** Mirrors the engine's add run: absent matched games are created, existing ones are untouched. */
  function confirmAdd() {
    const lib = snapshot.library;
    const dialog = lib?.addDialog;
    if (!lib || !dialog) return;
    const run = nextBatchRun();
    const picked = pickedCollections(dialog);
    const items: AddSummary['items'] = [];
    const created: Array<{ gameId: number; entryId: string }> = [];
    let added = 0;
    let alreadyPresent = 0;
    let skipped = 0;
    let failed = 0;
    for (const target of dialog.targets) {
      let row: Row | undefined;
      let gameId: number | null = null;
      const addTarget = target.target;
      if ('row' in addTarget) {
        row = rows.find((item) => item.key === addTarget.row);
        gameId = row ? getMatchedGameId(row.status) : null;
      } else {
        gameId = addTarget.game;
        row = rows.find((item) => getMatchedGameId(item.status) === gameId);
      }
      const title = row?.title ?? target.title;
      if (gameId == null) {
        skipped += 1;
        items.push({ target: target.target, entryKey: target.entryKey, title, outcome: 'skipped', status: null });
        continue;
      }
      if (row?.gamenode || accountLibraryGameIds.has(gameId)) {
        alreadyPresent += 1;
        items.push({
          target: target.target,
          entryKey: target.entryKey,
          title,
          outcome: 'alreadyPresent',
          status: row?.gamenode?.status ?? null,
        });
        continue;
      }
      if (options.failMutations) {
        failed += 1;
        items.push({ target: target.target, entryKey: target.entryKey, title, outcome: 'failed', status: null });
        continue;
      }
      const entryId = `entry-${gameId}-${run}`;
      if (row) {
        row.gamenode = {
          status: dialog.appliedStatus,
          collectionIds: [...picked],
          entryId,
        };
      }
      accountLibraryGameIds.add(gameId);
      created.push({ gameId, entryId });
      added += 1;
      items.push({
        target: target.target,
        entryKey: target.entryKey,
        title,
        outcome: 'added',
        status: dialog.appliedStatus,
      });
    }
    lastAdd = {
      runId: run,
      undoable: created.length > 0,
      items,
      added,
      alreadyPresent,
      skipped,
      failed,
    };
    createdByLastAdd = created;
    lib.addSummary = lastAdd;
    lib.addDialog = { ...dialog, summary: lastAdd };
  }

  /** Mirrors Undo add: delete exactly the entries the latest run created. */
  function undoAdd(run: number) {
    if (!lastAdd || lastAdd.runId !== run || !lastAdd.undoable) return;
    if (options.failMutations) {
      lastUndo = {
        run,
        state: 'failed',
        deleted: 0,
        failed: Math.max(1, createdByLastAdd.length),
      };
      lastAdd = { ...lastAdd, undoable: false };
      return;
    }
    for (const created of createdByLastAdd) {
      const row = rows.find((item) => getMatchedGameId(item.status) === created.gameId);
      if (row) row.gamenode = null;
      accountLibraryGameIds.delete(created.gameId);
    }
    lastUndo = { run, state: 'done', deleted: createdByLastAdd.length, failed: 0 };
    lastAdd = { ...lastAdd, undoable: false };
    createdByLastAdd = [];
  }

  /** Mirrors the batch status run: only known in-library games change. */
  function setStatus(keys: string[], status: EntryStatus) {
    const run = nextBatchRun();
    let changed = 0;
    let failed = 0;
    let skippedUnmatched = 0;
    let skippedNotInLibrary = 0;
    let skippedUnknownStatus = 0;
    const seenKeys = new Set<string>();
    const seenGames = new Set<number>();
    for (const key of keys) {
      if (seenKeys.has(key)) continue;
      seenKeys.add(key);
      const row = rows.find((item) => item.key === key);
      const gameId = row ? getMatchedGameId(row.status) : null;
      if (gameId == null) {
        skippedUnmatched += 1;
        continue;
      }
      if (seenGames.has(gameId)) continue;
      seenGames.add(gameId);
      if (!row?.gamenode) {
        skippedNotInLibrary += 1;
        continue;
      }
      if (row.gamenode.status == null) {
        skippedUnknownStatus += 1;
        continue;
      }
      if (options.failMutations) {
        failed += 1;
        continue;
      }
      row.gamenode = { ...row.gamenode, status };
      changed += 1;
    }
    lastStatusRun = {
      run,
      status,
      changed,
      skipped: skippedUnmatched + skippedNotInLibrary + skippedUnknownStatus,
      failed,
      skippedUnmatched,
      skippedNotInLibrary,
      skippedUnknownStatus,
    };
  }

  /** Mirrors the batch collection run: keeps status, unions the chosen collection. */
  function addToCollection(keys: string[], collectionId: string) {
    const run = nextBatchRun();
    const collection = collections.find((item) => item.id === collectionId);
    let changed = 0;
    let failed = 0;
    let skippedUnmatched = 0;
    let skippedNotInLibrary = 0;
    let skippedUnknownStatus = 0;
    let skippedAlreadyInCollection = 0;
    const seenKeys = new Set<string>();
    const seenGames = new Set<number>();
    for (const key of keys) {
      if (seenKeys.has(key)) continue;
      seenKeys.add(key);
      const row = rows.find((item) => item.key === key);
      const gameId = row ? getMatchedGameId(row.status) : null;
      if (gameId == null) {
        skippedUnmatched += 1;
        continue;
      }
      if (seenGames.has(gameId)) continue;
      seenGames.add(gameId);
      if (!row?.gamenode) {
        skippedNotInLibrary += 1;
        continue;
      }
      if (row.gamenode.status == null) {
        skippedUnknownStatus += 1;
        continue;
      }
      if (row.gamenode.collectionIds.includes(collectionId)) {
        skippedAlreadyInCollection += 1;
        continue;
      }
      if (options.failMutations) {
        failed += 1;
        continue;
      }
      row.gamenode = {
        ...row.gamenode,
        collectionIds: [...row.gamenode.collectionIds, collectionId],
      };
      changed += 1;
    }
    lastCollectionRun = {
      run,
      collectionId,
      collectionName: collection?.name ?? collectionId,
      changed,
      skipped:
        skippedUnmatched +
        skippedNotInLibrary +
        skippedUnknownStatus +
        skippedAlreadyInCollection,
      failed,
      skippedUnmatched,
      skippedNotInLibrary,
      skippedUnknownStatus,
      skippedAlreadyInCollection,
    };
  }

  /** Mirrors the local-first favorite: every entry of the game changes, unmatched ones do not. */
  function setFavorites(keys: string[], favorite: boolean) {
    const seen = new Set<number>();
    for (const key of keys) {
      const row = rows.find((item) => item.key === key);
      const gameId = row ? getMatchedGameId(row.status) : null;
      if (gameId == null || seen.has(gameId)) continue;
      seen.add(gameId);
      rows = rows.map((item) =>
        getMatchedGameId(item.status) === gameId
          ? { ...item, isFavorite: favorite }
          : item,
      );
    }
  }

  function signIn() {
    accountGeneration += 1;
    for (const pending of pendingViewWrites.values()) clearTimeout(pending.timer);
    pendingViewWrites.clear();
    snapshot.auth = { signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } };
    rows = rowsForScenarios();
    syncTracking();
    accountLibraryGameIds.clear();
    for (const row of rows) {
      const gameId = getMatchedGameId(row.status);
      if (row.gamenode && gameId != null) accountLibraryGameIds.add(gameId);
    }
    accountLibraryGameIds.add(13);
    selectedKeys = new Set();
    focusedKey = null;
    viewError = null;
    viewWrite = null;
    lastSavedView = null;
    catalogOperation += 1;
    catalogQuery = '';
    catalogStatus = 'idle';
    catalogResults = [];
    catalogIncomplete = false;
    scanProgressOverride = null;
    snapshot.localGames.discoveryScanning = false;
    lastAdd = null;
    lastUndo = null;
    lastStatusRun = null;
    lastCollectionRun = null;
    createdByLastAdd = [];
    snapshot.library = createLibrary(rows, initialScope, queryProviders);
    updateLibrary();
  }
  if (!options.signedOut) signIn();
  syncStoreAccounts();
  if (!options.signedOut && options.startScanning) startMockScan();
  function copy() { return structuredClone(snapshot); }
  function updateLibrary() {
    const lib = snapshot.library;
    if (!lib) return;
    const filters = lib.layout.filters;
    const effectiveScope = effectiveScopeOf(lib.layout);
    const query = runLibraryQuery({
      rows,
      layout: lib.layout,
      effectiveScope,
      search: lib.search,
      now: NOW,
      providers: queryProviders,
      gamenodeAvailable: true,
    });
    lib.total = query.total;
    lib.installedTotal = query.installedTotal;
    lib.providers = providerCounts(rows, query.filterCounts.stores, filters.showHidden);
    lib.rows = structuredClone(query.rows);
    // The engine keeps the internal selection while filters, search or a scope mask it, and
    // projects only the rows in display order into `selected`, `focused`, actions and detail.
    const shown = lib.rows.map((row) => row.key);
    const shownSet = new Set(shown);
    lib.selected = shown.filter((key) => selectedKeys.has(key));
    lib.focused =
      focusedKey != null && shownSet.has(focusedKey) ? focusedKey : null;
    lib.filterCounts = structuredClone(query.filterCounts);
    lib.scopeCounts = query.scopeCounts;
    lib.groups = query.groups;
    lib.savedViews = savedViews.map((view) => ({
      id: view.id,
      name: view.name,
      count: viewRowCount(view),
      active: savedViewId(lib.layout.scope) === view.id,
    }));
    lib.collections = collections.map((collection) => ({
      id: collection.id,
      name: collection.name,
      count: rows.filter(
        (row) =>
          (filters.showHidden || !row.hidden) &&
          (row.gamenode?.collectionIds.includes(collection.id) ?? false),
      ).length,
    }));
    lib.viewError = viewError;
    lib.viewWrite = viewWrite;
    lib.lastSavedView = lastSavedView;
    lib.lastHidden = lastHidden;
    lib.addSummary = lastAdd;
    lib.undo = lastUndo;
    lib.statusRun = lastStatusRun;
    lib.collectionRun = lastCollectionRun;
    lib.canSaveView = !payloadIsDefault(currentViewPayload(lib));
    lib.titleSource = query.titleSource;
    lib.catalog = catalogView();
    lib.scanProgress =
      scanProgressOverride ?? mockScanProgress(rows, snapshot.localGames.discoveryScanning);
    lib.paletteIndex = mockPaletteIndex(rows);
    if (!scanProgressOverride) lib.scanning = false;
    const availability = filters.availability;
    const installedOnly =
      availability.installed && !availability.owned && !availability.shared;
    const installedScope = effectiveScope === 'installed';
    if (query.total === 0) {
      lib.empty = { noGames: { stores: queryProviders.map((provider) => provider.name) } };
    } else if ((installedOnly || installedScope) && query.installedInScope === 0) {
      // The Installed scope and the installed-only availability fact both hide every
      // uninstalled game; give them the actionable state rather than a generic no-match.
      lib.empty = {
        noInstalled: {
          provider:
            query.titleSource.kind === 'store'
              ? query.titleSource.id
              : filters.stores.length === 1
                ? filters.stores[0]
                : null,
        },
      };
    } else if (query.visibleCount === 0) {
      lib.empty = 'nothingMatches';
    } else {
      lib.empty = null;
    }
    const selected = lib.rows.filter((row) => selectedKeys.has(row.key));
    const one = selected.length === 1 ? selected[0] : undefined;
    if (!one) {
      const disabled = { disabled: 'nothingSelected' as const };
      if (selected.length === 0) {
        lib.actions = {
          selected: 0,
          play: disabled,
          install: disabled,
          matchGame: disabled,
          addToGamenode: disabled,
          changeMatch: false,
        };
      } else {
        const multiple = { disabled: 'multipleSelected' as const };
        const anyMatched = selected.some(
          (row) => getMatchedGameId(row.status) != null,
        );
        lib.actions = {
          selected: selected.length,
          play: multiple,
          install: multiple,
          matchGame: multiple,
          addToGamenode: anyMatched
            ? 'available'
            : { disabled: 'noMatchedGames' },
          changeMatch: false,
        };
      }
    } else {
      const matched = getMatchedGameId(one.status) != null;
      const status = one.status;
      lib.actions = {
        selected: 1,
        play: one.running
          ? { disabled: 'running' }
          : one.installed
            ? 'available'
            : { disabled: 'noLaunchRule' },
        install: one.installed
          ? { disabled: 'noInstallRule' }
          : 'available',
        matchGame: 'available',
        addToGamenode:
          one.gamenode != null || matched
            ? 'available'
            : { disabled: 'notMatched' },
        changeMatch:
          (typeof status === 'object' && 'matched' in status) ||
          status === 'noGame',
      };
    }
    lib.detail = one ? {
      row: one,
      running: one.running,
      installDir: `/games/${one.title}`,
      releaseYear: one.releaseYear ?? 2022,
      hasArtwork: true,
      membership: one.gamenode
        ? { inLibrary: one.gamenode.status ?? null }
        : 'notInLibrary',
      achievementBackend: one.provider === 'steam',
      achievements: one.provider === 'steam'
        ? {
            state: 'ready',
            unlocked: MOCK_ACHIEVEMENTS.filter((achievement) => achievement.unlocked).length,
            total: MOCK_ACHIEVEMENTS.length,
            latestUnlock: {
              id: 'ACH_FIRST',
              name: 'First steps',
              unlockedAt: NOW - 300,
            },
            lastSuccessAt: NOW - 60,
            failure: null,
          }
        : null,
      suggestions: suggestionsFor(one),
    } : null;
  }
  function publish() { updateLibrary(); const next = copy(); listeners.forEach((listener) => listener(next)); }
  function pruneCollapsedSelection() {
    const lib = snapshot.library;
    if (!lib || lib.layout.group === 'none') return;
    const collapsed = new Set(lib.layout.collapsedGroups[lib.layout.group] ?? []);
    if (collapsed.size === 0) return;
    const hidden = new Set(
      rows
        .filter((row) => collapsed.has(groupId(row, lib.layout.group)))
        .map((row) => row.key),
    );
    for (const key of hidden) selectedKeys.delete(key);
    if (focusedKey != null && hidden.has(focusedKey)) focusedKey = null;
  }

  /** Group-aware grid movement, matching `Selection::step_grouped` in the engine. */
  function moveFocus(
    direction: 'up' | 'down' | 'left' | 'right' | 'home' | 'end',
    extend: boolean,
  ) {
    const lib = snapshot.library;
    if (!lib || lib.rows.length === 0) return;
    const visible = lib.rows;
    const current = visible.findIndex((row) => row.key === lib.focused);
    const start = current >= 0 ? current : direction === 'up' || direction === 'end' ? visible.length - 1 : 0;
    let target = start;
    if (direction === 'home') target = 0;
    else if (direction === 'end') target = visible.length - 1;
    else if (lib.layout.view === 'grid' && lib.layout.group !== 'none') {
      const lines: Array<{ start: number; len: number }> = [];
      let lineStart = 0;
      for (const group of lib.groups ?? []) {
        if (group.collapsed) continue;
        let offset = 0;
        while (offset < group.count) {
          const len = Math.min(group.count - offset, Math.max(1, gridColumns));
          lines.push({ start: lineStart + offset, len });
          offset += len;
        }
        lineStart += group.count;
      }
      while (lineStart < visible.length) {
        const len = Math.min(visible.length - lineStart, Math.max(1, gridColumns));
        lines.push({ start: lineStart, len });
        lineStart += len;
      }
      const lineIndex = lines.findIndex((line) => start >= line.start && start < line.start + line.len);
      const line = lineIndex >= 0 ? lineIndex : lines.length - 1;
      const column = start - (lines[line]?.start ?? 0);
      if (direction === 'left' && column > 0) target = start - 1;
      else if (direction === 'right' && column + 1 < (lines[line]?.len ?? 1)) target = start + 1;
      else if (direction === 'up' && line > 0) {
        const previous = lines[line - 1];
        target = previous.start + Math.min(column, previous.len - 1);
      } else if (direction === 'down' && line + 1 < lines.length) {
        const nextLine = lines[line + 1];
        target = nextLine.start + Math.min(column, nextLine.len - 1);
      }
    } else {
      target = Math.max(0, Math.min(visible.length - 1, start + (direction === 'left' || direction === 'up' ? -1 : 1)));
    }
    const next = visible[target];
    if (!next) return;
    focusedKey = next.key;
    if (extend && current >= 0) {
      const [first, last] = current < target ? [current, target] : [target, current];
      selectedKeys = new Set(visible.slice(first, last + 1).map((row) => row.key));
    } else {
      selectedKeys = new Set([next.key]);
    }
  }
  function libraryInput(input: LibraryInput) {
    const lib = snapshot.library;
    if (!lib) return;
    if (typeof input === 'object') {
      if ('setSearch' in input) lib.search = input.setSearch;
      else if ('setView' in input) lib.layout.view = input.setView;
      else if ('setFilter' in input) {
        lib.layout.scope = 'all';
        lib.layout.filters.stores =
          input.setFilter === 'all' ? [] : [input.setFilter.provider];
      }
      else if ('setScope' in input) {
        const scope = input.setScope;
        if (typeof scope === 'object' && 'savedView' in scope) applyView(scope.savedView);
        else lib.layout.scope = scope;
      }
      else if ('setGroup' in input) {
        lib.layout.group = input.setGroup;
        pruneCollapsedSelection();
      }
      else if ('toggleGroup' in input && lib.layout.group !== 'none') {
        const collapsed = new Set(lib.layout.collapsedGroups[lib.layout.group] ?? []);
        if (collapsed.has(input.toggleGroup.id)) collapsed.delete(input.toggleGroup.id);
        else collapsed.add(input.toggleGroup.id);
        lib.layout.collapsedGroups[lib.layout.group] = Array.from(collapsed);
        pruneCollapsedSelection();
      }
      else if ('setAllGroupsCollapsed' in input && lib.layout.group !== 'none') {
        lib.layout.collapsedGroups[lib.layout.group] = input.setAllGroupsCollapsed
          ? (lib.groups ?? []).map((group) => group.id)
          : [];
        pruneCollapsedSelection();
      }
      else if ('setFilters' in input) lib.layout.filters = input.setFilters;
      else if ('saveView' in input) saveView(input.saveView.name);
      else if ('renameView' in input) renameView(input.renameView.id, input.renameView.name);
      else if ('deleteView' in input) deleteView(input.deleteView.id);
      else if ('applyView' in input) applyView(input.applyView.id);
      else if ('setHidden' in input) setHidden(input.setHidden.keys, input.setHidden.hidden);
      else if ('setFavorite' in input) {
        const row = rows.find((item) => item.key === input.setFavorite.key);
        const gameId = row ? getMatchedGameId(row.status) : null;
        if (gameId != null) rows = rows.map((item) => getMatchedGameId(item.status) === gameId ? { ...item, isFavorite: input.setFavorite.favorite } : item);
      } else if ('setFavorites' in input) {
        setFavorites(input.setFavorites.keys, input.setFavorites.favorite);
      } else if ('undoAdd' in input) {
        undoAdd(input.undoAdd.run);
      } else if ('setStatus' in input) {
        setStatus(input.setStatus.keys, input.setStatus.status);
      } else if ('addToCollection' in input) {
        addToCollection(input.addToCollection.keys, input.addToCollection.collectionId);
      }
      else if ('setSort' in input) lib.layout.sort = input.setSort;
      else if ('setColumns' in input) gridColumns = Math.max(1, input.setColumns);
      else if ('setDetailOpen' in input) lib.layout.detailOpen = input.setDetailOpen;
      else if ('setDetailWidth' in input) lib.layout.detailWidth = input.setDetailWidth;
      else if ('setSidebarWidth' in input) lib.layout.sidebarWidth = input.setSidebarWidth;
      else if ('setSidebarCollapsed' in input) lib.layout.sidebarCollapsed = input.setSidebarCollapsed;
      else if ('select' in input) {
        const { key, mode } = input.select;
        if (lib.rows.some((row) => row.key === key)) {
          if (mode === 'toggle' && selectedKeys.has(key)) {
            selectedKeys.delete(key);
          } else if (mode === 'toggle') {
            selectedKeys.add(key);
          } else {
            selectedKeys = new Set([key]);
          }
          focusedKey = key;
        }
      } else if ('move' in input && lib.rows.length) {
        moveFocus(input.move.direction, input.move.extend);
      } else if ('useSuggestion' in input) {
        useSuggestion(input.useSuggestion.key, input.useSuggestion.gameId);
      } else if ('catalogFocus' in input) {
        catalogFocus = input.catalogFocus;
      } else if ('catalogSearch' in input) {
        catalogSearch(input.catalogSearch);
      } else if ('openAddForGame' in input) {
        openAddForGame(
          input.openAddForGame.gameId,
          input.openAddForGame.title,
          input.openAddForGame.coverUrl,
        );
      } else if ('setMatchQuery' in input && lib.matchDialog) lib.matchDialog.query = input.setMatchQuery;
      else if ('pickMatch' in input && lib.matchDialog) {
        const row = rows.find((item) => item.key === lib.matchDialog?.entryKey);
        if (row) { row.status = { matched: { gameId: input.pickMatch.gameId, source: 'chosen' } }; row.matchedName = row.title; }
        lib.matchDialog = null;
      } else if ('markNoGame' in input && lib.matchDialog) {
        const row = rows.find((item) => item.key === lib.matchDialog?.entryKey);
        if (row) {
          row.status = 'noGame';
          row.matchedName = null;
        }
        lib.matchDialog = null;
      } else if ('markEntryNoGame' in input) {
        // The detail pane's direct action: only the single selected unmatched entry changes.
        const { key } = input.markEntryNoGame;
        const row = rows.find((item) => item.key === key);
        if (lib.selected.length === 1 && lib.selected[0] === key && row?.status === 'unmatched') {
          row.status = 'noGame';
          row.matchedName = null;
          if (lib.matchDialog?.entryKey === key) lib.matchDialog = null;
        }
      } else if ('setAddStatus' in input && lib.addDialog) lib.addDialog.status = input.setAddStatus;
      else if ('toggleAddCollection' in input && lib.addDialog && typeof lib.addDialog.collections === 'object' && 'loaded' in lib.addDialog.collections) {
        const collection = lib.addDialog.collections.loaded.find((item) => item.id === input.toggleAddCollection);
        if (collection) collection.picked = !collection.picked;
      }
    } else if (input === 'selectAll') {
      const visible = lib.rows.map((row) => row.key);
      selectedKeys = new Set(visible);
      // With nothing focused (or the focused row masked), the first visible row gets the focus.
      if (focusedKey == null || !visible.includes(focusedKey)) {
        focusedKey = visible[0] ?? null;
      }
    } else if (input === 'clearSelection') {
      selectedKeys = new Set();
      focusedKey = null;
    } else if (input === 'openDetails') lib.layout.detailOpen = true;
    else if (input === 'catalogClear') catalogClear();
    else if (input === 'closeMatch') lib.matchDialog = null;
    else if (input === 'openMatch') {
      const row = rows.find((item) => item.key === lib.focused);
      if (row) lib.matchDialog = { entryKey: row.key, detectedName: row.detectedName, providerName: row.providerName, query: row.title, searching: false, candidates: [{ gameId: 100, name: row.title, current: false }], results: [], noResults: false, current: row.status, canClear: typeof row.status === 'object' };
    } else if (input === 'searchMatch' && lib.matchDialog) {
      lib.matchDialog.results = [{ gameId: 100, name: lib.matchDialog.query, current: false }];
    } else if (input === 'openAdd') {
      lib.addDialog = {
        targets: lib.rows
          .filter((row) => selectedKeys.has(row.key))
          .map((row) => ({
            target: { row: row.key },
            entryKey: row.key,
            title: row.title,
            matched: typeof row.status === 'object',
          })),
        mode: 'library',
        editingExisting: false,
        status: 'playing',
        appliedStatus: 'playing',
        appliedFrom: null,
        collections: { loaded: [{ id: 'favorites', name: 'Favorites', picked: false }] },
        progress: null,
        error: null,
        canConfirm: true,
        blocked: null,
        summary: null,
      };
    } else if (input === 'closeAdd') lib.addDialog = null;
    else if (input === 'confirmAdd') confirmAdd();
    reconcileViewHighlight();
  }
  return {
    subscribe(listener: Listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    subscribePortProgress(listener: ProgressListener) {
      progressListeners.add(listener);
      return () => { progressListeners.delete(listener); };
    },
    /** Plugin notifications, as the host's `plugin:notification` event delivers them. */
    subscribePluginNotices(listener: PluginNoticeListener) {
      pluginNoticeListeners.add(listener);
      return () => { pluginNoticeListeners.delete(listener); };
    },
    /** Lets a test or the browser preview publish a notification as a fictional plugin. */
    publishPluginNotice(notice: PluginNotice) {
      pluginNoticeListeners.forEach((listener) => listener(notice));
    },
    /** Confirmations a plugin asks of the user, as the host's `plugin:confirmation` event does. */
    subscribePluginConfirmations(listener: PluginConfirmationListener) {
      confirmationListeners.add(listener);
      return () => { confirmationListeners.delete(listener); };
    },
    subscribePluginConfirmationsClosed(listener: PluginConfirmationClosedListener) {
      confirmationClosedListeners.add(listener);
      return () => { confirmationClosedListeners.delete(listener); };
    },
    /** Lets a test or the browser preview ask for a confirmation as a fictional plugin. */
    publishPluginConfirmation(confirmation: PluginConfirmation) {
      openConfirmations.set(confirmation.id, confirmation);
      confirmationListeners.forEach((listener) => listener(confirmation));
    },
    /** The host stopping to wait for a confirmation (withdrawn or timed out). */
    closePluginConfirmation(id: string) {
      openConfirmations.delete(id);
      confirmationClosedListeners.forEach((listener) => listener(id));
    },
    subscribeRootSelections(listener: PluginRootPromptListener) {
      rootSelectionListeners.add(listener);
      return () => { rootSelectionListeners.delete(listener); };
    },
    subscribeRootConfirmations(listener: PluginRootPromptListener) {
      rootConfirmationListeners.add(listener);
      return () => { rootConfirmationListeners.delete(listener); };
    },
    subscribeRootPromptsClosed(listener: PluginRootPromptClosedListener) {
      rootPromptClosedListeners.add(listener);
      return () => { rootPromptClosedListeners.delete(listener); };
    },
    publishRootSelection(prompt: RootSelectionPrompt) {
      openRootSelections.set(prompt.id, prompt);
      rootSelectionListeners.forEach((listener) => listener(prompt));
    },
    publishRootConfirmation(prompt: RootConfirmationPrompt) {
      openRootConfirmations.set(prompt.id, prompt);
      rootConfirmationListeners.forEach((listener) => listener(prompt));
    },
    closeRootPrompt(id: string) {
      openRootSelections.delete(id);
      openRootConfirmations.delete(id);
      rootPromptClosedListeners.forEach((listener) => listener(id));
    },
    async invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (cmd === 'get_snapshot') return copy();
      if (cmd === 'plugin_update_preferences') return { periodicChecks: periodicPluginChecks };
      if (cmd === 'plugin_update_available' || cmd === 'plugin_update_check') return mockUpdates();
      if (cmd === 'plugin_update_set_periodic_checks') {
        periodicPluginChecks = args.enabled === true;
        return { periodicChecks: periodicPluginChecks };
      }
      if (cmd === 'plugin_rollback_available') {
        return mockRollbackVersion && mockPluginVersion === '2.0.0' ? [{
          pluginId: mockUpdatePlugin, name: 'Community Save Sync', activeVersion: mockPluginVersion, versions: [mockRollbackVersion],
        }] : [];
      }
      if (cmd === 'plugin_update_review' || cmd === 'plugin_rollback_review') {
        const rollback = cmd === 'plugin_rollback_review';
        const version = rollback ? mockRollbackVersion : mockUpdates()[0]?.versions[0];
        if (!version || args.pluginId !== mockUpdatePlugin || args.version !== version) throw new Error('Package version is unavailable');
        const playtime = { identifiesGameplay: false, identifiesPauses: false, withoutPauseDetection: 'disabled' } as const;
        mockUpdateReview = {
          pluginId: mockUpdatePlugin, installedVersion: mockPluginVersion, candidateVersion: version,
          addedPermissions: rollback ? [] : ['network.fetch'], removedPermissions: rollback ? ['network.fetch'] : [],
          changedPermissions: [], permissionChanges: [], addedNativeAccess: [], removedNativeAccess: [],
          apiChanged: false, installedApi: '1.0.0', candidateApi: '1.0.0',
          licenseChanged: false, installedLicense: 'MIT', candidateLicense: 'MIT',
          installedLicenseFiles: ['LICENSE'], candidateLicenseFiles: ['LICENSE'],
          installedLicenseNotices: [], candidateLicenseNotices: [],
          installedPlaytime: playtime, candidatePlaytime: playtime, playtimeChanged: false,
          publisherChanged: false, installedPublisherKeyId: 'mock-publisher', candidatePublisherKeyId: 'mock-publisher',
          publisherFingerprint: mockPublisher, changedDataSchemas: [], preservesPriorData: true,
          isDowngrade: rollback, requiresManualApproval: true, compatibilityDetails: mockCompatibility,
        };
        return mockUpdateReview;
      }
      if (cmd === 'plugin_update_approve' || cmd === 'plugin_rollback_approve') {
        const rollback = cmd === 'plugin_rollback_approve';
        const review = mockUpdateReview;
        if (!review || args.pluginId !== review.pluginId || args.version !== review.candidateVersion
          || review.isDowngrade !== rollback || (rollback && args.confirmed !== true)) throw new Error('Review and confirm the package first');
        mockUpdateReview = undefined;
        mockRollbackVersion = mockPluginVersion;
        mockPluginVersion = review.candidateVersion;
        return mockPluginVersion;
      }
      if (cmd === 'plugin_update_decline') {
        if (args.pluginId === mockUpdateReview?.pluginId) mockUpdateReview = undefined;
        return undefined;
      }
      if (cmd === 'available_plugin_extensions') return [];
      if (cmd === 'start_extension_action') return undefined;
      if (cmd === 'plugin_confirmation_answer') {
        const id = String(args.id ?? '');
        const asked = openConfirmations.get(id);
        openConfirmations.delete(id);
        if (asked && args.approved === true && args.remember === true) {
          const list = remembered.get(asked.pluginId) ?? [];
          list.push({
            id: `remembered-${list.length + 1}`,
            gameName: asked.gameName,
            operation: asked.operation,
            approvedAt: Math.floor(Date.now() / 1000),
          });
          remembered.set(asked.pluginId, list);
        }
        return undefined;
      }
      if (cmd === 'plugin_folder_selection_answer') {
        openRootSelections.delete(String(args.id ?? ''));
        return undefined;
      }
      if (cmd === 'plugin_folder_approval_answer') {
        openRootConfirmations.delete(String(args.id ?? ''));
        return undefined;
      }
      if (cmd === 'plugin_folder_approvals') {
        return structuredClone(folderApprovals.get(String(args.pluginId ?? '')) ?? []);
      }
      if (cmd === 'plugin_folder_revoke') {
        const pluginId = String(args.pluginId ?? '');
        const before = folderApprovals.get(pluginId) ?? [];
        const current = before.find((item) => item.id === args.id);
        if (!current || current.revoked) return false;
        if (current.logicalLibrary) {
          folderApprovals.set(pluginId, before.filter((item) => item.id !== args.id));
        } else {
          folderApprovals.set(pluginId, before.map((item) => item.id === args.id ? { ...item, revoked: true } : item));
        }
        return true;
      }
      if (cmd === 'plugin_folder_change') return undefined;
      if (cmd === 'plugin_library_root_set_watching') {
        const pluginId = String(args.pluginId ?? '');
        const before = folderApprovals.get(pluginId) ?? [];
        const current = before.find((item) => item.id === args.id && item.logicalLibrary);
        if (!current) return false;
        folderApprovals.set(pluginId, before.map((item) => item.id === args.id ? { ...item, watching: args.watching === true } : item));
        return true;
      }
      if (cmd === 'plugin_remembered_confirmations') {
        return [...(remembered.get(String(args.pluginId ?? '')) ?? [])];
      }
      if (cmd === 'plugin_revoke_confirmation') {
        const pluginId = String(args.pluginId ?? '');
        const before = remembered.get(pluginId) ?? [];
        const after = before.filter((item) => item.id !== args.id);
        remembered.set(pluginId, after);
        return after.length !== before.length;
      }
      if (cmd === 'plugin_inventory') {
        const platform = options.platform ?? 'windows';
        const steamPlugin: PluginInfo = {
          providerId: 'steam',
          pluginId: 'org.gamenode.plugin.steam-discovery',
          pluginKind: 'provider',
          name: 'Steam',
          source: 'official',
          status: 'installed',
          capabilities: [
            'provider.context.read', 'library.discovery', 'library.catalog',
            'account.state.read', 'account.state.publish', 'auth.webview',
            'auth.webview.cookies.read', 'auth.webview.page.read', 'secrets.read',
            'secrets.write', 'secrets.delete', 'playtime.counter',
            'provider.actions.declare',
          ],
          session: scenarios.steam === 'notConnected' ? 'stopped' : 'active',
          nativeAccess: platform === 'windows'
            ? ['files', 'network', 'registry']
            : ['files', 'network'],
        };
        const epicPlugin: PluginInfo = {
          providerId: 'epic',
          pluginId: 'org.gamenode.plugin.epic',
          pluginKind: 'provider',
          name: 'Epic Games Store',
          source: 'official',
          status: 'installed',
          capabilities: [
            'provider.context.read', 'library.discovery', 'library.catalog',
            'account.state.read', 'account.state.publish', 'auth.webview',
            'auth.webview.page.read', 'secrets.read', 'secrets.write',
            'secrets.delete', 'playtime.counter', 'provider.actions.declare',
          ],
          session: scenarios.epic === 'notConnected' ? 'stopped' : 'active',
          nativeAccess: ['files', 'network', 'registry'],
        };
        const communityPlugin: PluginInfo = {
          providerId: 'community-save-sync',
          pluginId: 'community.example.save-sync',
          pluginKind: 'extension',
          name: 'Community Save Sync',
          version: mockPluginVersion,
          compatibilityDetails: mockCompatibility,
          source: 'community',
          status: 'installed',
          capabilities: ['library.read', 'files.read', 'libraries.roots.write'],
          session: 'active',
          nativeAccess: ['files', 'network'],
          publisherFingerprint: mockPublisher,
        };
        if (platform === 'linux') {
          const packageManagerPlugin: PluginInfo = {
            providerId: 'packagekit',
            pluginId: 'org.gamenode.plugin.packagekit-discovery',
            pluginKind: 'provider',
            name: 'Gerenciador de pacotes Linux',
            source: 'official',
            status: 'installed',
            capabilities: ['provider.context.read', 'library.discovery'],
            session: 'stopped',
            nativeAccess: ['files', 'process', 'dbus'],
          };
          return [steamPlugin, packageManagerPlugin, communityPlugin];
        }
        return [steamPlugin, epicPlugin, communityPlugin];
      }
      if (cmd === 'get_app_info') {
        return {
          locale: options.locale ?? 'en',
          version: 'mock',
          backgroundSupported: false,
          platform: options.platform ?? 'windows',
          logDirectory: '/tmp/gamenode-logs',
        };
      }
      if (cmd === 'get_license_notices') return 'Mock UI development session';
      if (cmd === 'get_recent_logs') return '2026-01-01T00:00:00Z INFO gamenode_app: mock log line';
      if (cmd === 'open_logs_folder') return undefined;
      if (cmd === 'dispatch_auth') {
        const input = args.input as AuthInput;
        if (input === 'signOut') {
          accountGeneration += 1;
          for (const pending of pendingViewWrites.values()) clearTimeout(pending.timer);
          pendingViewWrites.clear();
          selectedKeys = new Set();
          focusedKey = null;
          viewError = null;
          viewWrite = null;
          lastSavedView = null;
          snapshot.auth = 'signedOut';
          snapshot.library = null;
          snapshot.tracking = {
            title: '',
            available: true,
            running: [],
            sessionStartedAt: {},
          };
          appliedViewPayload = null;
          appliedViewId = null;
        } else if (typeof input === 'object' && 'startSignIn' in input) signIn();
      } else if (cmd === 'library_input') libraryInput(args.input as LibraryInput);
      else if (cmd === 'remove_entry') {
        const key = args.key as string;
        rows = rows.filter((row) => row.key !== key);
        selectedKeys.delete(key);
        if (focusedKey === key) focusedKey = null;
      } else if (cmd === 'remove_local_game') { /* the mock has no DRM-free registrations */ }
      else if (cmd === 'set_sync_enabled') snapshot.sync.enabled = args.enabled as boolean;
      else if (cmd === 'dismiss_sync_explanation') snapshot.sync.explained = true;
      else if (cmd === 'set_playtime_source') snapshot.playtimeSource = args.source as Snapshot['playtimeSource'];
      else if (cmd === 'provider_sign_in') {
        const provider = args.providerId as StoreId;
        if (!storeIds.includes(provider)) return undefined;
        previousScenarios[provider] = scenarios[provider] === 'signingIn' ? 'notConnected' : scenarios[provider];
        scenarios[provider] = 'signingIn';
        syncStoreAccounts();
        publish();
        scheduleStore(provider, 1500, () => {
          rows = rowsForScenarios();
          pruneSelectionToRows();
          scenarios[provider] = 'connected';
          syncStoreAccounts();
          publish();
        });
      } else if (cmd === 'provider_cancel_sign_in') {
        const provider = args.providerId as StoreId;
        if (!storeIds.includes(provider)) return undefined;
        generations[provider] += 1;
        scenarios[provider] = scenarios[provider] === 'signingIn' ? previousScenarios[provider] : scenarios[provider];
        syncStoreAccounts();
        publish();
      } else if (cmd === 'provider_focus_sign_in') {
        // The mock has no separate window to focus.
      } else if (cmd === 'provider_sign_out') {
        const provider = args.providerId as StoreId;
        if (!storeIds.includes(provider)) return undefined;
        generations[provider] += 1;
        clearCatalog(provider);
        scenarios[provider] = 'notConnected';
        syncStoreAccounts();
        publish();
      } else if (cmd === 'provider_check') {
        const provider = args.providerId as StoreId;
        if (!storeIds.includes(provider)) return undefined;
        scenarios[provider] = 'checking';
        syncStoreAccounts();
        publish();
        scheduleStore(provider, 900, () => {
          rows = rowsForScenarios();
          pruneSelectionToRows();
          scenarios[provider] = 'connected';
          syncStoreAccounts();
          publish();
        });
      } else if (cmd === 'rescan_library') {
        startMockScan();
      } else if (cmd === 'open_gamenode_page') {
        console.info('[UI mock] openGameNodePage', args.page);
      } else if (cmd === 'update_window_geometry' || cmd === 'dismiss_launch') { /* no external effects */ }
      else if (cmd === 'ports_open') {
        if (ports.catalog.state !== 'ready' && ports.catalog.state !== 'refreshing') {
          refreshPortCatalog();
        }
      } else if (cmd === 'ports_refresh') {
        refreshPortCatalog();
      } else if (cmd === 'achievement_list') {
        const key = (args.key as string | undefined) ?? '';
        const filter = (args.filter as string | undefined) ?? 'all';
        if (!key.startsWith('steam:')) return null;
        return mockAchievementList(filter);
      } else if (cmd === 'ports_detail') {
        const entry = ports.entries.find((item) => item.catalogId === args.catalogId);
        return entry ? mockDetailFor(entry) : null;
      } else if (cmd === 'ports_pick_parent') {
        const folder = ports.preferences.lastParentFolder ?? 'D:\\Ports';
        applyPortPreferences({ lastParentFolder: folder });
        return folder;
      } else if (cmd === 'ports_pick_supplied') {
        const request = args.request as PortPickSuppliedRequest;
        const path =
          request.kind === 'package'
            ? `D:\\Dumps\\${request.update ? 'update' : 'base'}-${request.slotId}.pkg`
            : request.kind === 'folder'
              ? `D:\\Dumps\\${request.slotId}`
              : `D:\\Dumps\\${request.slotId}.iso`;
        emitPortProgress({
          jobId: request.setupId,
          phase: request.kind === 'package' ? 'extractingPackage' : 'hashing',
          slotId: request.slotId,
          bytesDone: 1,
          bytesTotal: 1,
        });
        // Hollow Mountain's package slot models the mistake this fixture exists for: a folder
        // that holds `.pkg` files picked in folder mode. The host names the package option.
        if (request.kind === 'folder' && request.slotId === 'game-package') {
          return {
            slotId: request.slotId,
            path,
            status: {
              rejected: {
                message: 'the folder contains game packages; use the package option instead',
                acceptedLabels: ['CUSA20202 (01.01)'],
                packagesFound: true,
              },
            },
          } satisfies PortSuppliedPick;
        }
        return {
          slotId: request.slotId,
          path,
          status: 'accepted',
        } satisfies PortSuppliedPick;
      } else if (cmd === 'ports_install') {
        return startPortInstall(args.request as PortInstallRequest);
      } else if (cmd === 'ports_cancel') {
        cancelPortJob(args.jobId as string);
      } else if (cmd === 'ports_mark_seen') {
        markPortsSeen(args.catalogId as string);
      } else if (cmd === 'ports_set_preferences') {
        applyPortPreferences(args.patch as PortPreferencesPatch);
      } else if (cmd === 'ports_pick_executable') {
        const job = findPortJob(args.jobId as string);
        return `D:\\Ports\\${job?.slug ?? 'port'}\\port.exe`;
      } else if (cmd === 'ports_tools_cache') {
        return mockPortToolsCache();
      } else if (cmd === 'ports_clear_cache') {
        return clearMockPortCache();
      }
      else if (cmd === 'open_install_folder') { /* the mock has no file manager */ }
      else { console.warn(`[UI mock] Unsupported command: ${cmd}`); return; }
      publish();
    },
  };
}

function isMockSteamScenario(value: string | null): value is MockSteamScenario {
  return value === 'notConnected' || value === 'checking' || value === 'signingIn'
    || value === 'connected' || value === 'expired' || value === 'unreachable'
    || value === 'credentialError' || value === 'persistFailed'
    || value === 'catalogExpired' || value === 'catalogUnreachable';
}

function isMockCatalogScenario(
  value: string | null,
): value is 'default' | 'error' | 'offline' | 'incomplete' {
  return value === 'default' || value === 'error' || value === 'offline' || value === 'incomplete';
}

/** `?steam=expired` (or any `MockSteamScenario`) selects a store-account fixture. */
const requestedSteamScenario = new URLSearchParams(window.location.search).get('steam');
const requestedParams = new URLSearchParams(window.location.search);
const requestedCatalogScenario = requestedParams.get('catalog');

export const mockHost = createMockHost({
  signedOut: requestedParams.get('scenario') === 'signed-out',
  firstRun: requestedParams.get('scenario') === 'first-run',
  locale: requestedParams.get('locale') === 'pt-BR' ? 'pt-BR' : 'en',
  storeAccount: isMockSteamScenario(requestedSteamScenario) ? requestedSteamScenario : undefined,
  largeLibrary: requestedParams.get('library') === 'large',
  variantLibrary: requestedParams.get('library') === 'variants',
  emptyLibrary: requestedParams.get('library') === 'empty',
  noInstalledLibrary: requestedParams.get('library') === 'no-installed',
  discoveries: requestedParams.get('discoveries') === '1',
  startScanning: requestedParams.get('scanning') === '1',
  viewWriteFailure:
    requestedParams.get('viewWrite') === 'fail-once'
      ? 'once'
      : requestedParams.get('viewWrite') === 'fail'
        ? 'always'
        : 'none',
  // PC ports are Windows-only in this release. The mock is a Windows signed-in build by
  // default so the destination is visible in a browser on any OS; `?platform=linux`
  // exercises the hidden case.
  platform: requestedParams.get('platform') === 'linux' ? 'linux' : 'windows',
  portsScenario: requestedParams.get('ports') === 'unavailable' ? 'unavailable' : 'default',
  catalogScenario: isMockCatalogScenario(requestedCatalogScenario)
    ? requestedCatalogScenario
    : undefined,
  failMutations: requestedParams.get('mutations') === 'fail',
});
