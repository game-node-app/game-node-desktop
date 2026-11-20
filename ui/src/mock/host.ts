/** An isolated, in-memory host for interactive UI development. */
import {
  createDefaultFilters,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultSnapshot,
  filtersAreDefault,
  getMatchedGameId,
  type AddItem,
  type AddSummary,
  type AppPreferencePatch,
  type AppPreferences,
  type AuthInput,
  type BackupUsageGame,
  type CatalogStatus,
  type CollectionRunView,
  type LibraryInput,
  type LibraryLayout,
  type LibraryScope,
  type LibraryView,
  type EntryStatus,
  type MatchOption,
  type PaletteEntry,
  type PluginCompatibilityDetails,
  type PluginInfo,
  type PluginUpdateReview,
  type Provider,
  type Registration,
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
  type ToolCatalogItemView,
  type ToolCacheEntryView,
  type ToolChannel,
  type ToolModalView,
  type ToolRelease,
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

/**
 * Creating a collection with this name is refused with `collection_limit_reached`, so the
 * inline limit message can be exercised in `VITE_UI_MOCK=1`.
 */
export const MOCK_LIMIT_COLLECTION_NAME = 'Collection limit';

/** A hidden fixture entry, so Show hidden and the hidden counts can be exercised. */
export const MOCK_HIDDEN_KEY = 'steam:900';

/**
 * The one game whose tools open with an anti-cheat detection, so the danger callout and the
 * disabled Install button are reachable through the usual library action in the mock.
 */
export const MOCK_TOOLS_ANTICHEAT_KEY = 'steam:9';

const MOCK_TOOL_CATALOG: ToolCatalogItemView[] = [
  {
    id: 'optiscaler',
    name: 'OptiScaler',
    summary: 'Upscaler & frame generation middleware (DLSS / XeSS / FSR 2/3/4)',
    license: 'GPL-3.0',
    sourceUrl: 'https://github.com/optiscaler/OptiScaler',
  },
];

const MOCK_TOOL_RELEASES: ToolRelease[] = [
  {
    toolId: 'optiscaler',
    channel: 'stable',
    version: '0.7.9',
    tagName: 'v0.7.9',
    name: 'OptiScaler 0.7.9',
    publishedAt: '2026-05-01',
    downloadUrl:
      'https://github.com/optiscaler/OptiScaler/releases/download/v0.7.9/OptiScaler.7z',
    assetName: 'OptiScaler.7z',
    sizeBytes: 39845888,
    sha256: 'sha-0.7.9',
    cachedPath: '/cache/optiscaler-0.7.9.7z',
  },
  {
    toolId: 'optiscaler',
    channel: 'stable',
    version: '0.7.2',
    tagName: 'v0.7.2',
    name: 'OptiScaler 0.7.2',
    publishedAt: '2026-04-01',
    downloadUrl:
      'https://github.com/optiscaler/OptiScaler/releases/download/v0.7.2/OptiScaler.7z',
    assetName: 'OptiScaler.7z',
    sizeBytes: 36000000,
    sha256: null,
    cachedPath: '/cache/optiscaler-0.7.2.7z',
  },
  {
    toolId: 'optiscaler',
    channel: 'nightly',
    version: '0.8.0-pre1',
    tagName: 'nightly-2026-06-01',
    name: 'OptiScaler Nightly 2026-06-01',
    publishedAt: '2026-06-01',
    downloadUrl:
      'https://github.com/optiscaler/OptiScaler/releases/download/nightly/OptiScaler-nightly.7z',
    assetName: 'OptiScaler-nightly.7z',
    sizeBytes: 41000000,
    sha256: 'sha-nightly',
    cachedPath: '/cache/optiscaler-nightly.7z',
  },
];

/** The Settings › Ferramentas cache fixture: three archives, one without a published digest. */
const MOCK_TOOL_CACHE_ENTRIES: ToolCacheEntryView[] = MOCK_TOOL_RELEASES.map((release) => ({
  toolId: release.toolId,
  version: release.version,
  channel: release.channel,
  bytes: release.sizeBytes,
  sha256Verified: release.sha256 != null,
  inUse: false,
}));

/**
 * Cached cover file names for the signed-out mural. Fewer than 25, so the remaining tiles
 * exercise the neutral fallback, and all resolve to `/mock-covers/` in the mock.
 */
export const MOCK_COVER_WALL = Array.from({ length: 12 }, (_, index) => `mock-${index}.svg`);

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
const HIDDEN_SORT_LABEL = 'Hidden achievement';

/** 58 achievements with 31 unlocked, one locked hidden and rarity on most rows (D04). */
const MOCK_ACHIEVEMENTS = Array.from({ length: 58 }, (_, index) => ({
  id: `ACH_${String(index).padStart(2, '0')}`,
  name: index === 40 ? 'A secret' : `Achievement ${index + 1}`,
  description: index === 40 ? 'A spoiler description.' : `Fictional milestone ${index + 1}.`,
  hidden: index === 40,
  unlocked: index < 31,
  unlockedAt: index < 31 ? NOW - 300 - index * 3_600 : null,
  globalPercent: index % 7 === 6 ? null : 120 + index * 41,
  position: index,
}));

function achievementSortKey(
  achievement: (typeof MOCK_ACHIEVEMENTS)[number],
): string {
  return achievement.hidden && !achievement.unlocked
    ? HIDDEN_SORT_LABEL
    : achievement.name;
}

function mockAchievementList(filter: string, sort: string) {
  const records = MOCK_ACHIEVEMENTS
    .filter((achievement) =>
      filter === 'unlocked'
        ? achievement.unlocked
        : filter === 'locked'
          ? !achievement.unlocked
          : true,
    )
    .sort((left, right) => {
      if (sort === 'rarest' || sort === 'mostCommon') {
        const leftMissing = left.globalPercent == null;
        const rightMissing = right.globalPercent == null;
        if (leftMissing !== rightMissing) return leftMissing ? 1 : -1;
        if (!leftMissing && !rightMissing && left.globalPercent !== right.globalPercent) {
          return sort === 'rarest'
            ? left.globalPercent! - right.globalPercent!
            : right.globalPercent! - left.globalPercent!;
        }
        return left.position - right.position;
      }
      if (sort === 'name') {
        return achievementSortKey(left)
          .toLocaleLowerCase()
          .localeCompare(achievementSortKey(right).toLocaleLowerCase()) ||
          left.position - right.position;
      }
      // Most recent: unlocked newest first, unlocked without a time, then locked.
      const group = (achievement: typeof left): number =>
        achievement.unlocked
          ? achievement.unlockedAt == null
            ? 1
            : 0
          : 2;
      const groupOrder = group(left) - group(right);
      if (groupOrder !== 0) return groupOrder;
      if (group(left) === 0) return right.unlockedAt! - left.unlockedAt!;
      return left.position - right.position;
    })
    .map((achievement) => ({
      id: achievement.id,
      name: achievement.name,
      description: achievement.description,
      hidden: achievement.hidden,
      iconUrl: '',
      iconGrayUrl: '',
      unlocked: achievement.unlocked,
      unlockedAt: achievement.unlockedAt,
      globalPercent: achievement.globalPercent,
      position: achievement.position,
    }));
  const unlocked = MOCK_ACHIEVEMENTS.filter((achievement) => achievement.unlocked).length;
  return {
    connected: true,
    state: 'ready' as const,
    lastSuccessAt: NOW - 60,
    refreshedAt: NOW - 60,
    failure: null,
    counts: {
      all: MOCK_ACHIEVEMENTS.length,
      unlocked,
      locked: MOCK_ACHIEVEMENTS.length - unlocked,
    },
    hasRarity: MOCK_ACHIEVEMENTS.some((achievement) => achievement.globalPercent != null),
    providerName: 'Steam',
    hasProviderPage: true,
    achievements: records,
  };
}

const MOCK_PROVIDERS: ProviderRef[] = [
  {
    id: 'steam',
    name: 'Steam',
    installRoot: 'C:\\Program Files (x86)\\Steam',
    libraryFolders: 4,
  },
  // GOG and Epic report no location, so the "detected, no path" state is reachable.
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

/**
 * Three match candidates at 96/74/38%, as in the design's D01 fixture. A matched row's current
 * game replaces the first option so the dialog preselects it; every other row gets generated ids.
 */
function mockMatchCandidates(row: Row): MatchOption[] {
  const matched = getMatchedGameId(row.status);
  const year = row.releaseYear ?? 2024;
  return [
    {
      gameId: matched ?? 1000,
      name: matched != null ? (row.matchedName ?? row.title) : row.title,
      releaseYear: year,
      coverUrl: `/mock-covers/mock-${(row.key.length + 1) % 12}.svg`,
      coverFile: `mock-${(row.key.length + 1) % 12}.svg`,
      current: matched != null,
      confidence: 96,
      tone: 'success',
      platforms: ['PC', 'Switch'],
      reasons: [
        { titleSimilarity: { percent: 96 } },
        'sameYear',
        'storeIdRecognized',
      ],
    },
    {
      gameId: 1001,
      name: `${row.title} Remastered`,
      releaseYear: 2021,
      coverUrl: `/mock-covers/mock-${(row.key.length + 4) % 12}.svg`,
      coverFile: null,
      current: false,
      confidence: 74,
      tone: 'warning',
      platforms: ['PC'],
      reasons: [
        { titleSimilarity: { percent: 74 } },
        'storeIdRecognized',
      ],
    },
    {
      gameId: 1002,
      name: `${row.title} Classic`,
      releaseYear: 2019,
      coverUrl: null,
      coverFile: null,
      current: false,
      confidence: 38,
      tone: 'neutral',
      platforms: ['PC', 'PS4'],
      reasons: [
        { titleSimilarity: { percent: 38 } },
        'storeIdRecognized',
      ],
    },
  ];
}

/** The search answer's results, in the server's order and without the candidates. */
function mockMatchResults(query: string): MatchOption[] {
  return [
    {
      gameId: 2000,
      name: query,
      releaseYear: 2020,
      coverUrl: null,
      coverFile: null,
      current: false,
      confidence: 60,
      tone: 'warning',
      platforms: ['PC'],
      reasons: [{ titleSimilarity: { percent: 60 } }],
    },
    {
      gameId: 2001,
      name: `${query} II`,
      releaseYear: 2024,
      coverUrl: null,
      coverFile: null,
      current: false,
      confidence: 30,
      tone: 'neutral',
      platforms: [],
      reasons: [{ titleSimilarity: { percent: 30 } }],
    },
  ];
}

function mockPlaytimeLabel(seconds: number | null | undefined): string | null {
  if (seconds == null || seconds < 0) return null;
  const hours = Math.floor(seconds / 3600);
  if (hours > 0) return `${hours} h`;
  return `${Math.floor((seconds % 3600) / 60)} min`;
}

/** The two UI languages the host supports, from any raw OS locale. */
function mockNormalizeLocale(raw: string): string {
  return raw.trim().toLowerCase().replace('_', '-').startsWith('pt') ? 'pt-BR' : 'en';
}

/** The language the host would use for the current preference. */
function mockEffectiveLocale(
  preferences: AppPreferences,
  systemLocale: string,
): string {
  if (preferences.locale === 'ptBr') return 'pt-BR';
  if (preferences.locale === 'en') return 'en';
  return systemLocale;
}

/** Applies one tagged preference patch, with the same validation the host performs. */
function applyMockAppPreference(
  preferences: AppPreferences,
  patch: AppPreferencePatch,
): AppPreferences {
  const entries = patch && typeof patch === 'object' ? Object.entries(patch) : [];
  if (entries.length !== 1) throw 'invalidPreference';
  const [key, value] = entries[0];
  const next = { ...preferences, sharedExcluded: [...preferences.sharedExcluded] };
  switch (key) {
    case 'startWithWindows':
    case 'startMinimized':
    case 'closeToTray':
    case 'showStatusBar':
    case 'discoveryOnStart':
    case 'discoveryRealtime':
    case 'toolAutoCheck':
      if (typeof value !== 'boolean') throw 'invalidPreference';
      next[key] = value;
      break;
    case 'locale':
      if (!['system', 'en', 'ptBr'].includes(value as string)) throw 'invalidPreference';
      next.locale = value as AppPreferences['locale'];
      break;
    case 'startScope':
      if (!['last', 'all', 'installed', 'favorites', 'recent'].includes(value as string)) {
        throw 'invalidPreference';
      }
      next.startScope = value as AppPreferences['startScope'];
      break;
    case 'toolDefaultChannel':
      if (!['stable', 'nightly'].includes(value as string)) throw 'invalidPreference';
      next.toolDefaultChannel = value as ToolChannel;
      break;
    case 'sharedExcluded':
      if (
        !Array.isArray(value) ||
        value.some((id) => typeof id !== 'string' || !/^[A-Za-z0-9._-]{1,64}$/.test(id))
      ) {
        throw 'invalidPreference';
      }
      next.sharedExcluded = [...new Set(value)];
      break;
    default:
      throw 'invalidPreference';
  }
  return next;
}

/** Every non-hidden row, independent of filters, for the command palette. */
function mockPaletteIndex(source: Row[], sharedExcluded: string[] = []): PaletteEntry[] {
  return source
    .filter(
      (row) =>
        !row.hidden &&
        !(
          row.shared &&
          !row.owned &&
          !row.installed &&
          sharedExcluded.includes(row.provider)
        ),
    )
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
  sharedExcluded: string[] = [],
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
    sharedExcluded,
  });
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    layout,
    search: '',
    access: 'verified',
    total: query.total,
    installedTotal: query.installedTotal,
    providers: providerCounts(
      source,
      query.filterCounts.stores,
      layout.filters.showHidden,
      providers,
    ),
    rows: structuredClone(query.rows),
    filterCounts: structuredClone(query.filterCounts),
    scopeCounts: query.scopeCounts,
    groups: query.groups,
    titleSource: query.titleSource,
    catalog: { query: '', status: 'idle', results: [], incomplete: false },
    scanProgress: mockScanProgress(source, false),
    paletteIndex: mockPaletteIndex(source, sharedExcluded),
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

/** The tracking conflict behind the DRM-free `?claim=1` fixture. */
function applyClaimFixture(local: Snapshot['localGames']) {
  local.pendingClaim = {
    storeKey: 'steam:9',
    storeEntry: {
      title: 'Infinite Coast',
      provider: 'steam',
      coverFile: 'mock-8.svg',
      playtimeSeconds: 8 * 3600 + 16 * 60,
    },
    registration: {
      title: 'Infinite Coast',
      root: 'D:\\Jogos\\Infinite Coast',
      coverFile: 'mock-2.svg',
    },
  };
}

/**
 * The two watched folders behind the Settings › Jogos DRM-free states: one
 * available and one whose drive is disconnected.
 */
function applyWatchFixture(local: Snapshot['localGames']) {
  local.watches = [
    {
      id: 'watched-games',
      path: 'D:\\Jogos',
      enabled: true,
      entryCount: 2,
      lastCheckedAt: NOW - 12 * 60,
      state: 'idle',
      unavailableSince: null,
    },
    {
      id: 'watched-itch',
      path: 'E:\\Itch',
      enabled: true,
      entryCount: 6,
      lastCheckedAt: NOW - 26 * 3_600,
      state: 'unavailable',
      unavailableSince: NOW - 3 * 3_600,
    },
  ];
}

/** The DRM-free registrations, profile, backups and profile history behind the variant rows. */
function applyVariantLocalGames(local: Snapshot['localGames']) {
  applyWatchFixture(local);
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
      customCover: null,
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
      customCover: null,
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
      retention: 10,
    },
  ];
  local.versions = [
    {
      installation: 'brasa',
      profile: 'brasa-profile',
      destination: 'D:\\Backups\\Brasa',
      id: 'brasa-backup-12',
      label: '2026-11-15 14:20:00',
      bytes: 7_100_000,
      createdAt: NOW - 2 * 3_600,
      reason: 'afterSession',
      playSeconds: 42 * 60,
    },
    {
      installation: 'brasa',
      profile: 'brasa-profile',
      destination: 'D:\\Backups\\Brasa',
      id: 'brasa-backup-11',
      label: '2026-11-14 22:05:00',
      bytes: 7_000_000,
      createdAt: NOW - 26 * 3_600,
      reason: 'afterSession',
      playSeconds: 72 * 60,
    },
    {
      installation: 'brasa',
      profile: 'brasa-profile',
      destination: 'D:\\Backups\\Brasa',
      id: 'brasa-backup-10',
      label: '2026-11-14 18:30:00',
      bytes: 6_800_000,
      createdAt: NOW - 30 * 3_600,
      reason: 'manual',
      playSeconds: null,
    },
    {
      installation: 'brasa',
      profile: 'brasa-profile_recovery_mock',
      destination: 'D:\\Backups\\Brasa',
      id: 'brasa-recovery-1',
      label: '2026-11-10 21:44:00',
      bytes: 6_800_000,
      createdAt: NOW - 5 * DAY,
      reason: 'recovery',
      playSeconds: null,
    },
  ];
  local.historyBytes = local.versions.reduce((total, version) => total + version.bytes, 0);
  local.historyCount = local.versions.length;
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
  local.settings = { automatic: true, destination: 'D:\\Backups', retention: 10 };
  local.discoveryCompleted = true;
  local.discoveryLastCheckedAt = NOW - 12 * 60;
}

function candidateFileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? '';
}

/** Mirrors `identity::maintenance` in the core, including the `redist` infix. */
function maintenanceName(name: string): boolean {
  const lower = candidateFileName(name).toLowerCase().replace(/\.exe$/, '');
  return (
    [
      'setup',
      'install',
      'unins',
      'update',
      'redist',
      'vcredist',
      'dxsetup',
      'support',
      'unitycrashhandler',
      'crashreport',
      'crashhandler',
      'crashpad',
      'ue4prereq',
      'ue5prereq',
    ].some((word) => lower.startsWith(word)) ||
    lower.includes('redist') ||
    [
      'editor',
      'config',
      'settings',
      'benchmark',
      'dedicatedserver',
      'crashreporter',
      'updater',
    ].some((word) => lower.endsWith(word))
  );
}

function candidateLikelyNonGame(
  candidate: Snapshot['localGames']['candidates'][number],
): boolean {
  if (candidate.evidence.some((evidence) => evidence.strong)) return false;
  const executable = candidate.selectedTarget ?? candidate.targets[0] ?? '';
  return maintenanceName(executable) || maintenanceName(candidate.root);
}

/** Three watched-folder candidates for the discovered banner and its review (`?discoveries=1`). */
function applyDiscoveryFixture(local: Snapshot['localGames']) {
  const candidates: Snapshot['localGames']['candidates'] = [
    {
      fingerprint: 'candidate-moss-machine',
      watch: 'watched-games',
      root: 'D:\\Jogos\\MossMachine',
      title: 'Moss & Machine',
      targets: ['D:\\Jogos\\MossMachine\\MossMachine.exe'],
      selectedTarget: 'D:\\Jogos\\MossMachine\\MossMachine.exe',
      evidence: [{ kind: 'product', value: 'Moss & Machine', strong: true }],
      ignored: false,
      incomplete: false,
      suggestion: {
        gameId: 1201,
        title: 'Moss & Machine',
        year: 2023,
        coverUrl: '/mock-covers/mock-4.svg',
        confidence: 96,
        tone: 'success',
      },
      likelyNonGame: false,
    },
    {
      fingerprint: 'candidate-frostbyte-arcade',
      watch: 'watched-games',
      root: 'D:\\Jogos\\FrostbyteArcade',
      title: 'Frostbyte Arcade',
      targets: ['D:\\Jogos\\FrostbyteArcade\\frostbyte.exe'],
      selectedTarget: 'D:\\Jogos\\FrostbyteArcade\\frostbyte.exe',
      evidence: [{ kind: 'product', value: 'Frostbyte Arcade', strong: true }],
      ignored: false,
      incomplete: false,
      suggestion: null,
      likelyNonGame: false,
    },
    {
      fingerprint: 'candidate-setup-redist',
      watch: 'watched-games',
      root: 'D:\\Jogos\\Instaladores',
      title: 'setup_redist',
      targets: ['D:\\Jogos\\Instaladores\\vc_redist.x64.exe'],
      selectedTarget: 'D:\\Jogos\\Instaladores\\vc_redist.x64.exe',
      evidence: [],
      ignored: false,
      incomplete: false,
      suggestion: null,
      likelyNonGame: true,
    },
  ];
  local.candidates = candidates;
  local.recentlyAdded = candidates.map((candidate) => candidate.title);
  local.reviewSelected = candidates
    .filter((candidate) => !candidate.likelyNonGame)
    .map((candidate) => candidate.fingerprint);
  local.watches = local.watches.length > 0
    ? local.watches
    : [{
        id: 'watched-games',
        path: 'D:\\Jogos',
        enabled: true,
        entryCount: 0,
        lastCheckedAt: NOW - 12 * 60,
        state: 'idle',
        unavailableSince: null,
      }];
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

/** The fixture tools modal for one game, as the host's inspection would return it. */
function createMockToolModal(
  entryKey: string,
  title: string,
  installDir: string,
): ToolModalView {
  const candidates = [
    {
      relativeExe: 'bin/x64/Game.exe',
      label: 'Shipping Executable',
      architecture: 'x64',
      preferred: true,
    },
    {
      relativeExe: 'bin/x64/Launcher.exe',
      label: 'Launcher',
      architecture: 'x64',
      preferred: false,
    },
  ];
  const detected = entryKey === MOCK_TOOLS_ANTICHEAT_KEY;
  return {
    entryKey,
    title,
    installDir,
    inspection: {
      entryKey,
      installDir,
      candidates,
      selectedExe: candidates[0].relativeExe,
      anticheat: detected
        ? {
            detected: ['easyAntiCheat'],
            evidence: ['EasyAntiCheat/EasyAntiCheat_x64.dll'],
          }
        : { detected: [], evidence: [] },
    },
    tools: [],
    activeToolId: 'optiscaler',
    selectedChannel: 'stable',
    availableVersions: MOCK_TOOL_RELEASES.filter(
      (release) => release.channel === 'stable',
    ),
    selectedVersion: '0.7.9',
    selectedProxy: 'dxgi',
    anticheatConfirmed: false,
    operation: null,
  };
}

export function createMockHost(options: {
  signedOut?: boolean;
  /** Signed-in first-run: no library, no connected store and no watched folder. */
  firstRun?: boolean;
  locale?: string;
  /** Steam account fixture. */
  storeAccount?: MockSteamScenario;
  /** Epic account fixture, so the expired-Epic state is reachable on its own. */
  epicAccount?: MockSteamScenario;
  steamStorage?: StorageMethod;
  /** Seeds the two watched folders, including the unavailable `E:\\Itch` drive. */
  watches?: 'unavailable';
  largeLibrary?: boolean;
  /** Comprehensive L6 fixture: every match state plus registered DRM-free rows. */
  variantLibrary?: boolean;
  /** Signed-in first-run fixture with no library rows. */
  emptyLibrary?: boolean;
  /** Owned and family-shared rows that are all not installed; starts on Installed. */
  noInstalledLibrary?: boolean;
  /** Watched-folder candidates for the discovered banner and review. */
  discoveries?: boolean;
  /** A pending DRM-free tracking conflict for the claim dialog. */
  claim?: boolean;
  /** Starts a rescan on load, for the scanning panel and skeleton states. */
  startScanning?: boolean;
  /** How a saved-view storage write fails: never, once, or every time. */
  viewWriteFailure?: 'none' | 'once' | 'always';
  platform?: PortPlatform;
  portsScenario?: 'default' | 'unavailable';
  catalogScenario?: 'default' | 'error' | 'offline' | 'incomplete';
  failMutations?: boolean;
  /** Milliseconds one mock add item takes; 0 keeps confirmAdd synchronous for tests. */
  addBatchDelayMs?: number;
  /** Keeps a started sign-in pending, so the browser-reopen dialog can be exercised. */
  signInPending?: boolean;
  /** Makes Open again report a failure while the attempt keeps waiting. */
  signInReopenFails?: boolean;
  /** Signed-in account whose first-sync explanation has not been dismissed yet. */
  syncFirstRun?: boolean;
  /** What the mock executable picker does: accept a file, cancel, report a failure, or reject a path outside the game folder. */
  toolPick?: 'accept' | 'cancel' | 'fail' | 'outside';
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
  const systemLocale = mockNormalizeLocale(options.locale ?? 'en');
  const effectiveLocale = () => mockEffectiveLocale(snapshot.preferences, systemLocale);
  snapshot.tools.catalog = MOCK_TOOL_CATALOG;
  snapshot.tools.cacheEntries = MOCK_TOOL_CACHE_ENTRIES.map((entry) => ({ ...entry }));
  snapshot.tools.totalCacheBytes = snapshot.tools.cacheEntries.reduce(
    (total, entry) => total + entry.bytes,
    0,
  );
  snapshot.tools.totalCacheArchives = snapshot.tools.cacheEntries.length;
  // Two installed games use the cached 0.7.9 archive, so Ferramentas shows
  // "Em 2 jogos" and the cache row starts marked In use.
  snapshot.tools.installedByEntry = Object.fromEntries(
    ['epic:6', 'epic:8'].map((entryKey) => [
      entryKey,
      [
        {
          toolId: 'optiscaler',
          name: 'OptiScaler',
          channel: 'stable' as ToolChannel,
          version: '0.7.9',
          proxyDll: 'dxgi.dll',
          targetExe: 'bin/x64/Launcher.exe',
          state: 'installed' as const,
          summary: 'Installed version 0.7.9',
        },
      ],
    ]),
  );
  refreshMockToolCacheInUse();
  if (options.syncFirstRun) {
    // First successful sign-in on this PC: the explanation is still pending.
    snapshot.sync = { enabled: true, explained: false, summary: null, running: false };
  }
  // The status bar fixture: the watched folders were checked 12 minutes ago.
  snapshot.localGames.discoveryLastCheckedAt = NOW - 12 * 60;
  if (options.variantLibrary) applyVariantLocalGames(snapshot.localGames);
  if (options.watches === 'unavailable') applyWatchFixture(snapshot.localGames);
  if (options.discoveries) applyDiscoveryFixture(snapshot.localGames);
  if (options.claim) applyClaimFixture(snapshot.localGames);
  if (options.firstRun) {
    snapshot.localGames.watches = [];
    snapshot.localGames.registrations = [];
    snapshot.localGames.profiles = [];
    snapshot.localGames.versions = [];
    snapshot.localGames.historyBytes = 0;
    snapshot.localGames.historyCount = 0;
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
  /** The GameNode cover each DRM-free row had before a custom cover replaced it in the mock. */
  const mockGameNodeCovers = new Map<string, string | null>();
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
  const collections: Array<{ id: string; name: string }> =
    options.firstRun || options.emptyLibrary ? [] : MOCK_COLLECTIONS.map((c) => ({ ...c }));
  let createdCollections = 0;
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
    epic: options.firstRun ? 'notConnected' : options.epicAccount ?? 'connected',
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
  /** Plugin process sessions the Settings › Plugins cards show; Stop/Start mutate them. */
  const pluginSessions: Record<string, 'active' | 'stopped'> = {
    steam: scenarios.steam === 'notConnected' ? 'stopped' : 'active',
    epic: scenarios.epic === 'notConnected' ? 'stopped' : 'active',
    packagekit: 'stopped',
    // The community plugin fixture starts stopped, so the Start path is visible.
    community: 'stopped',
  };

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
    if (reopenAddRun()) return;
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
          coverFile: null,
          matched: true,
        },
      ],
      mode: 'catalog',
      editingExisting: false,
      status: 'playing',
      appliedStatus: 'playing',
      appliedFrom: null,
      collections: { loaded: addCollectionOptions([]) },
      progress: null,
      items: [],
      error: null,
      canConfirm: true,
      blocked: null,
      summary: null,
      newCollection: null,
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
      sharedExcluded: snapshot.preferences.sharedExcluded,
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

  /** Mirrors the engine: the first picked collection with a default decides the status. */
  function refreshAppliedAddStatus(dialog: NonNullable<LibraryView['addDialog']>) {
    const collections = dialog.collections;
    const picked = typeof collections === 'object' && 'loaded' in collections
      ? collections.loaded.filter((collection) => collection.picked)
      : [];
    const deciding = picked.find((collection) => collection.defaultStatus != null);
    dialog.appliedStatus = deciding?.defaultStatus ?? dialog.status;
    dialog.appliedFrom = deciding?.name ?? null;
  }

  function pickedCollections(dialog: NonNullable<LibraryView['addDialog']>): string[] {
    const collections = dialog.collections;
    if (typeof collections !== 'object' || !('loaded' in collections)) return [];
    return collections.loaded.filter((collection) => collection.picked).map((collection) => collection.id);
  }

  /** The collections the dialog lists, with their counts from the mock account library. */
  function addCollectionOptions(picked: string[]): Array<{
    id: string;
    name: string;
    defaultStatus: EntryStatus | null;
    picked: boolean;
    entryCount: number | null;
  }> {
    return collections.map((collection) => ({
      id: collection.id,
      name: collection.name,
      defaultStatus: null,
      picked: picked.includes(collection.id),
      entryCount: rows.filter((row) =>
        row.gamenode?.collectionIds.includes(collection.id),
      ).length,
    }));
  }

  /** A batch add that outlives its dialog, mirroring the engine's single active run. */
  interface MockAddRun {
    runId: number;
    items: AddItem[];
    gameIds: Array<number | null>;
    targets: NonNullable<LibraryView['addDialog']>['targets'];
    mode: NonNullable<LibraryView['addDialog']>['mode'];
    editingExisting: boolean;
    status: EntryStatus;
    appliedStatus: EntryStatus;
    picked: string[];
    next: number;
    created: Array<{ gameId: number; entryId: string }>;
    timer: ReturnType<typeof setTimeout> | null;
  }

  /** How long one mock add item takes. Zero keeps the mock synchronous for tests. */
  const addStepDelay = options.addBatchDelayMs ?? 0;
  let addRun: MockAddRun | null = null;

  function addRunProgress(): { done: number; total: number } {
    if (!addRun) return { done: 0, total: 0 };
    return {
      done: addRun.items.filter((item) =>
        item.state !== 'waiting' && item.state !== 'adding').length,
      total: addRun.items.length,
    };
  }

  function syncAddRunIntoDialog() {
    const lib = snapshot.library;
    if (!lib?.addDialog || !addRun) return;
    lib.addDialog.items = addRun.items.map((item) => ({ ...item }));
    lib.addDialog.progress = addRunProgress();
  }

  function cancelAddRun() {
    if (addRun?.timer) clearTimeout(addRun.timer);
    addRun = null;
  }

  function finishAddRun() {
    const run = addRun;
    const lib = snapshot.library;
    if (!run) return;
    const items = run.items.map((item) => ({ ...item }));
    const added = items.filter((item) => item.state === 'added').length;
    const alreadyPresent = items.filter((item) => item.state === 'alreadyPresent').length;
    const skipped = items.filter((item) => item.state === 'skipped').length;
    const failed = items.filter((item) => item.state === 'failed').length;
    lastAdd = {
      runId: run.runId,
      undoable: run.created.length > 0,
      items,
      added,
      alreadyPresent,
      skipped,
      failed,
    };
    createdByLastAdd = run.created;
    addRun = null;
    if (lib) {
      lib.addSummary = lastAdd;
      if (lib.addDialog) {
        lib.addDialog = {
          ...lib.addDialog,
          items,
          progress: { done: items.length, total: items.length },
          summary: lastAdd,
          error: null,
          canConfirm: false,
        };
      }
    }
    // The notice survives the library view, so a batch that ends while the account changes is
    // still reported. A single-game run keeps its previous silent behavior unless it added.
    snapshot.addNotice = {
      accountId: 'mock-user',
      batch: run.items.length > 1,
      summary: lastAdd,
    };
    publish();
  }

  /** Stops a hidden batch when the mock account signs out, counting the rest as not added. */
  function stopAddRunForSignOut() {
    const run = addRun;
    if (!run) return;
    if (run.timer) clearTimeout(run.timer);
    for (const item of run.items) {
      if (item.state === 'waiting' || item.state === 'adding') {
        item.state = 'failed';
        item.error = 'rejected';
      }
    }
    const items = run.items.map((item) => ({ ...item }));
    lastAdd = {
      runId: run.runId,
      undoable: run.created.length > 0,
      items,
      added: items.filter((item) => item.state === 'added').length,
      alreadyPresent: items.filter((item) => item.state === 'alreadyPresent').length,
      skipped: items.filter((item) => item.state === 'skipped').length,
      failed: items.filter((item) => item.state === 'failed').length,
    };
    createdByLastAdd = run.created;
    addRun = null;
    snapshot.addNotice = {
      accountId: 'mock-user',
      batch: run.items.length > 1,
      summary: lastAdd,
    };
  }

  function resolveAddItem(index: number) {
    const run = addRun;
    if (!run) return;
    const item = run.items[index];
    const gameId = run.gameIds[index];
    const row = gameId == null
      ? undefined
      : rows.find((candidate) => getMatchedGameId(candidate.status) === gameId);
    if (gameId == null) {
      item.state = 'skipped';
      return;
    }
    if (row?.gamenode || accountLibraryGameIds.has(gameId)) {
      item.state = 'alreadyPresent';
      item.status = row?.gamenode?.status ?? null;
      return;
    }
    if (options.failMutations) {
      item.state = 'failed';
      item.error = 'failed';
      return;
    }
    const entryId = `entry-${gameId}-${run.runId}`;
    if (row) {
      row.gamenode = {
        status: run.appliedStatus,
        collectionIds: [...run.picked],
        entryId,
      };
    }
    accountLibraryGameIds.add(gameId);
    run.created.push({ gameId, entryId });
    item.state = 'added';
    item.status = run.appliedStatus;
  }

  function advanceAddRun() {
    const run = addRun;
    const lib = snapshot.library;
    if (!run) return;
    if (!lib) {
      cancelAddRun();
      return;
    }
    if (run.next >= run.items.length) {
      finishAddRun();
      return;
    }
    const index = run.next;
    run.next += 1;
    const item = run.items[index];
    if (run.gameIds[index] == null) {
      item.state = 'skipped';
      syncAddRunIntoDialog();
      publish();
      advanceAddRun();
      return;
    }
    item.state = 'adding';
    syncAddRunIntoDialog();
    publish();
    const step = () => {
      if (!addRun) return;
      resolveAddItem(index);
      addRun.timer = null;
      syncAddRunIntoDialog();
      publish();
      advanceAddRun();
    };
    if (addStepDelay > 0) {
      run.timer = setTimeout(step, addStepDelay);
    } else {
      step();
    }
  }

  /** Reopens the running batch in the dialog without starting another run. */
  function reopenAddRun(): boolean {
    const lib = snapshot.library;
    const run = addRun;
    if (!lib || !run) return false;
    lib.addDialog = {
      targets: run.targets,
      mode: run.mode,
      editingExisting: run.editingExisting,
      status: run.status,
      appliedStatus: run.appliedStatus,
      appliedFrom: null,
      collections: { loaded: addCollectionOptions(run.picked) },
      progress: addRunProgress(),
      items: run.items.map((item) => ({ ...item })),
      error: null,
      canConfirm: false,
      blocked: null,
      summary: null,
      newCollection: null,
    };
    return true;
  }

  /** Mirrors the engine's add run: absent matched games are created, existing ones are untouched. */
  function mockToolEntry(entryKey: string | null | undefined) {
    const key =
      entryKey ??
      (snapshot.library?.selected.length === 1
        ? snapshot.library.selected[0]
        : null);
    if (!key) return null;
    const row = rows.find((candidate) => candidate.key === key);
    if (!row) return null;
    // The detail pane uses the same folder, so the target path and Browse agree.
    return { key, title: row.title, installDir: `/games/${row.title}` };
  }

  /** Opens the tools modal the same way the context menu and detail actions do. */
  function openMockTools(entryKey?: string | null) {
    const entry = mockToolEntry(entryKey);
    if (!entry) return;
    const modal = createMockToolModal(
      entry.key,
      entry.title,
      entry.installDir,
    );
    // Reopening a game keeps the tool it already installed, as the host's installations do.
    const installed = snapshot.tools.installedByEntry[entry.key] ?? [];
    if (installed.length > 0) {
      const tool = installed[0];
      modal.tools = installed;
      modal.selectedChannel = tool.channel;
      modal.selectedVersion = tool.version;
      modal.selectedProxy = (tool.proxyDll?.replace(/\.dll$/i, '') ??
        modal.selectedProxy) as ToolModalView['selectedProxy'];
    } else {
      // A game without an installation opens on the preferred default channel.
      modal.selectedChannel = snapshot.preferences.toolDefaultChannel;
      modal.availableVersions = MOCK_TOOL_RELEASES.filter(
        (release) => release.channel === modal.selectedChannel,
      );
      modal.selectedVersion = modal.availableVersions[0]?.version ?? null;
    }
    snapshot.tools.modal = modal;
  }

  function installedMockTool(modal: ToolModalView) {
    const version = modal.selectedVersion ?? modal.availableVersions[0]?.version;
    if (!version) return;
    const tool: ToolModalView['tools'][number] = {
      toolId: modal.activeToolId,
      name:
        snapshot.tools.catalog.find((item) => item.id === modal.activeToolId)
          ?.name ?? modal.activeToolId,
      channel: modal.selectedChannel,
      version,
      proxyDll: `${modal.selectedProxy}.dll`,
      targetExe: modal.inspection?.selectedExe ?? '',
      state: 'installed',
      summary: `Installed version ${version}`,
    };
    modal.tools = [tool];
    modal.operation = null;
    snapshot.tools.installedByEntry[modal.entryKey] = [tool];
    refreshMockToolCacheInUse();
  }

  /** Marks a cached archive in use when any game installation references its version. */
  function refreshMockToolCacheInUse() {
    const installed = Object.values(snapshot.tools.installedByEntry)
      .flat()
      .map((tool) => `${tool.toolId}\u0000${tool.version}`);
    for (const entry of snapshot.tools.cacheEntries) {
      entry.inUse = installed.includes(`${entry.toolId}\u0000${entry.version}`);
    }
  }

  function confirmAdd() {
    const lib = snapshot.library;
    const dialog = lib?.addDialog;
    if (!lib || !dialog || dialog.summary || addRun) return;
    const runId = nextBatchRun();
    const picked = pickedCollections(dialog);
    const items: AddItem[] = [];
    const gameIds: Array<number | null> = [];
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
      items.push({
        target: target.target,
        entryKey: target.entryKey,
        title: row?.title ?? target.title,
        state: 'waiting',
        status: null,
        error: null,
      });
      gameIds.push(gameId);
    }
    // A new run replaces the previous summary: only the latest add can be undone.
    lastAdd = null;
    createdByLastAdd = [];
    lib.addSummary = null;
    snapshot.addNotice = null;
    addRun = {
      runId,
      items,
      gameIds,
      targets: dialog.targets,
      mode: dialog.mode,
      editingExisting: dialog.editingExisting,
      status: dialog.status,
      appliedStatus: dialog.appliedStatus,
      picked,
      next: 0,
      created: [],
      timer: null,
    };
    lib.addDialog = {
      ...dialog,
      items: items.map((item) => ({ ...item })),
      progress: { done: 0, total: items.length },
      summary: null,
      error: null,
      canConfirm: false,
    };
    publish();
    advanceAddRun();
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

  function signIn(provider: Provider = 'google') {
    snapshot.notice = null;
    if (options.signInPending) {
      snapshot.auth = {
        pending: {
          provider,
          awaitingBrowser: true,
          canReopen: true,
          reopenFailed: false,
        },
      };
      return;
    }
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
    snapshot.library = createLibrary(
      rows,
      initialScope,
      queryProviders,
      snapshot.preferences.sharedExcluded,
    );
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
    const sharedExcluded = snapshot.preferences.sharedExcluded;
    const query = runLibraryQuery({
      rows,
      layout: lib.layout,
      effectiveScope,
      search: lib.search,
      now: NOW,
      providers: queryProviders,
      gamenodeAvailable: true,
      sharedExcluded,
    });
    lib.total = query.total;
    lib.installedTotal = query.installedTotal;
    lib.providers = providerCounts(
      rows,
      query.filterCounts.stores,
      filters.showHidden,
      queryProviders,
    );
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
          !(
            row.shared &&
            !row.owned &&
            !row.installed &&
            sharedExcluded.includes(row.provider)
          ) &&
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
    lib.paletteIndex = mockPaletteIndex(rows, sharedExcluded);
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
              id: 'ACH_00',
              name: 'Achievement 1',
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
      else if ('setCoverSize' in input) lib.layout.coverSize = input.setCoverSize;
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
      else if ('selectMatch' in input && lib.matchDialog) {
        const offered = [
          ...lib.matchDialog.candidates,
          ...lib.matchDialog.results,
        ].find((option) => option.gameId === input.selectMatch.gameId);
        if (offered) lib.matchDialog.selected = offered;
      } else if ('confirmMatch' in input && lib.matchDialog) {
        const selected = lib.matchDialog.selected;
        if (selected) {
          const row = rows.find((item) => item.key === lib.matchDialog?.entryKey);
          if (row) {
            row.status = { matched: { gameId: selected.gameId, source: 'chosen' } };
            row.matchedName = selected.name;
          }
          lib.matchDialog = null;
        }
      } else if ('pickMatch' in input && lib.matchDialog) {
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
      } else if ('setAddStatus' in input && lib.addDialog) {
        lib.addDialog.status = input.setAddStatus;
        refreshAppliedAddStatus(lib.addDialog);
      } else if ('toggleAddCollection' in input && lib.addDialog && typeof lib.addDialog.collections === 'object' && 'loaded' in lib.addDialog.collections) {
        const collection = lib.addDialog.collections.loaded.find((item) => item.id === input.toggleAddCollection);
        if (collection) collection.picked = !collection.picked;
        refreshAppliedAddStatus(lib.addDialog);
      }
      else if ('setNewCollectionName' in input && lib.addDialog?.newCollection) {
        const draft = lib.addDialog.newCollection;
        if (!draft.creating) {
          draft.name = input.setNewCollectionName;
          draft.canCreate = draft.name.trim().length > 0;
          draft.error = null;
        }
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
      if (row) {
        const candidates = mockMatchCandidates(row);
        lib.matchDialog = {
          entryKey: row.key,
          detectedName: row.detectedName,
          providerName: row.providerName,
          query: row.title,
          searching: false,
          candidates,
          results: [],
          selected: candidates[0] ?? null,
          noResults: false,
          current: row.status,
          canClear: typeof row.status === 'object',
        };
      }
    } else if (input === 'searchMatch' && lib.matchDialog) {
      lib.matchDialog.results = mockMatchResults(lib.matchDialog.query);
      if (!lib.matchDialog.selected) {
        const offered = [...lib.matchDialog.candidates, ...lib.matchDialog.results];
        lib.matchDialog.selected =
          offered.sort((left, right) => right.confidence - left.confidence)[0] ?? null;
      }
    } else if (input === 'openAdd') {
      // One run at a time: an Add while a batch is going reopens its progress.
      if (!reopenAddRun()) {
        lib.addDialog = {
          targets: lib.rows
            .filter((row) => selectedKeys.has(row.key))
            .map((row) => ({
              target: { row: row.key },
              entryKey: row.key,
              title: row.title,
              coverFile: row.coverFile,
              matched: typeof row.status === 'object',
            })),
          mode: 'library',
          editingExisting: false,
          status: 'playing',
          appliedStatus: 'playing',
          appliedFrom: null,
          collections: { loaded: addCollectionOptions([]) },
          progress: null,
          items: [],
          error: null,
          canConfirm: true,
          blocked: null,
          summary: null,
          newCollection: null,
        };
      }
    } else if (input === 'closeAdd') {
      // A batch keeps going after its dialog closes; a single-game run is abandoned.
      const run = addRun;
      if (!run || run.items.length <= 1) cancelAddRun();
      lib.addDialog = null;
    } else if (input === 'hideAdd') {
      const run = addRun;
      if (!run || run.items.length <= 1) cancelAddRun();
      lib.addDialog = null;
    } else if (input === 'startNewCollection') {
      if (lib.addDialog && !lib.addDialog.summary && !lib.addDialog.newCollection) {
        lib.addDialog.newCollection = {
          name: '',
          creating: false,
          canCreate: false,
          error: null,
        };
      }
    } else if (input === 'createCollection') {
      const dialog = lib.addDialog;
      const draft = dialog?.newCollection;
      const name = draft?.name.trim() ?? '';
      if (dialog && draft && draft.canCreate && name) {
        if (name === MOCK_LIMIT_COLLECTION_NAME) {
          draft.error = 'limitReached';
          draft.canCreate = true;
        } else if (options.failMutations) {
          draft.error = 'failed';
          draft.canCreate = true;
        } else {
          draft.creating = true;
          draft.canCreate = false;
          const finish = () => {
            createdCollections += 1;
            const id = `collection-${createdCollections}`;
            collections.push({ id, name });
            const current = snapshot.library?.addDialog;
            if (!current?.newCollection) return;
            const picked = [...pickedCollections(current), id];
            current.collections = { loaded: addCollectionOptions(picked) };
            current.newCollection = null;
            publish();
          };
          if (addStepDelay > 0) setTimeout(finish, addStepDelay);
          else finish();
        }
      }
    } else if (input === 'cancelNewCollection') {
      if (lib.addDialog) lib.addDialog.newCollection = null;
    } else if (input === 'confirmAdd') confirmAdd();
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
          version: '0.1.0',
          source: 'official',
          status: 'installed',
          capabilities: [
            'provider.context.read', 'library.discovery', 'library.catalog',
            'account.state.read', 'account.state.publish', 'auth.webview',
            'auth.webview.cookies.read', 'auth.webview.page.read', 'secrets.read',
            'secrets.write', 'secrets.delete', 'playtime.counter',
            'provider.actions.declare',
          ],
          session: pluginSessions.steam,
          nativeAccess: platform === 'windows'
            ? ['files', 'network', 'registry']
            : ['files', 'network'],
        };
        const epicPlugin: PluginInfo = {
          providerId: 'epic',
          pluginId: 'org.gamenode.plugin.epic',
          pluginKind: 'provider',
          name: 'Epic Games Store',
          version: '0.1.0',
          source: 'official',
          status: 'installed',
          capabilities: [
            'provider.context.read', 'library.discovery', 'library.catalog',
            'account.state.read', 'account.state.publish', 'auth.webview',
            'auth.webview.page.read', 'secrets.read', 'secrets.write',
            'secrets.delete', 'playtime.counter', 'provider.actions.declare',
          ],
          session: pluginSessions.epic,
          nativeAccess: ['files', 'network', 'registry'],
        };
        const communityPlugin: PluginInfo = {
          providerId: 'community',
          pluginId: 'dev.prateleira.local',
          pluginKind: 'provider',
          name: 'Prateleira Local',
          version: '0.3.1',
          source: 'community',
          status: 'installed',
          capabilities: ['provider.context.read'],
          session: pluginSessions.community,
          nativeAccess: platform === 'linux' ? ['files', 'dbus'] : ['files', 'network'],
        };
        const communitySaveSyncPlugin: PluginInfo = {
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
            version: '0.1.0',
            source: 'official',
            status: 'installed',
            capabilities: ['provider.context.read', 'library.discovery'],
            session: pluginSessions.packagekit,
            nativeAccess: ['files', 'process', 'dbus'],
          };
          return [steamPlugin, packageManagerPlugin, communityPlugin, communitySaveSyncPlugin];
        }
        return [steamPlugin, epicPlugin, communityPlugin, communitySaveSyncPlugin];
      }
      if (cmd === 'plugin_stop' || cmd === 'plugin_start') {
        const providerId = args.providerId as string;
        if (!(providerId in pluginSessions)) {
          throw new Error(`unknown provider: ${providerId}`);
        }
        pluginSessions[providerId] = cmd === 'plugin_stop' ? 'stopped' : 'active';
        const account = snapshot.storeAccounts.find(
          (candidate) => candidate.provider === providerId,
        );
        if (account) {
          if (cmd === 'plugin_stop') {
            // Account and sync actions are unavailable until the plugin starts again.
            account.canSignIn = false;
            account.canFocusSignIn = false;
            account.canCancelSignIn = false;
            account.canCheck = false;
            account.canSignOut = false;
            account.canRemoveCredential = false;
          } else {
            syncStoreAccounts();
          }
        }
        publish();
        return undefined;
      }
      if (cmd === 'get_app_info') {
        return {
          locale: effectiveLocale(),
          systemLocale,
          version: 'mock',
          buildDate: '2026-01-01',
          osVersion: (options.platform ?? 'windows') === 'windows'
            ? 'Windows 11 Pro (build 26100)'
            : 'Linux',
          arch: 'x86_64',
          backgroundSupported: (options.platform ?? 'windows') === 'windows',
          platform: options.platform ?? 'windows',
          logDirectory: '/tmp/gamenode-logs',
        };
      }
      if (cmd === 'system_info_text') {
        const platform = options.platform ?? 'windows';
        const plugins = platform === 'linux'
          ? [
              ['Steam', '0.1.0', 'official'],
              ['Gerenciador de pacotes Linux', '0.1.0', 'official'],
              ['Prateleira Local', '0.3.1', 'community'],
            ]
          : [
              ['Steam', '0.1.0', 'official'],
              ['Epic Games Store', '0.1.0', 'official'],
              ['Prateleira Local', '0.3.1', 'community'],
            ];
        const lines = [
          'GameNode Desktop mock',
          'Build 2026-01-01',
          `${platform === 'windows' ? 'Windows 11 Pro (build 26100)' : 'Linux'} x86_64`,
          `Locale: ${effectiveLocale()}`,
          '',
          'Plugins:',
          ...plugins.map(([name, version, source]) => `- ${name} ${version} (${source}, installed)`),
          '',
          'Detected stores:',
          '- Steam: 5 installed',
          ...(platform === 'windows' ? ['- Epic Games Store: 2 installed'] : []),
        ];
        return lines.join('\n');
      }
      if (cmd === 'open_gamenode_link') {
        console.info('[UI mock] openGameNodeLink', args.kind);
        return undefined;
      }
      if (cmd === 'get_license_notices') return 'Mock UI development session';
      if (cmd === 'get_recent_logs') return '2026-01-01T00:00:00Z INFO gamenode_app: mock log line';
      if (cmd === 'open_logs_folder') return undefined;
      if (cmd === 'dispatch_auth') {
        const input = args.input as AuthInput;
        if (input === 'signOut') {
          stopAddRunForSignOut();
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
        } else if (typeof input === 'object' && 'startSignIn' in input) {
          signIn(input.startSignIn);
        } else if (input === 'reopenBrowser') {
          if (
            typeof snapshot.auth === 'object' &&
            snapshot.auth !== null &&
            'pending' in snapshot.auth
          ) {
            snapshot.auth = {
              pending: {
                ...snapshot.auth.pending,
                reopenFailed: options.signInReopenFails ?? false,
              },
            };
          }
        } else if (input === 'cancelSignIn') {
          if (
            typeof snapshot.auth === 'object' &&
            snapshot.auth !== null &&
            'pending' in snapshot.auth
          ) {
            snapshot.auth = 'signedOut';
            snapshot.notice = 'signInCancelled';
          }
        }
      } else if (cmd === 'sign_in_cover_wall') {
        return [...MOCK_COVER_WALL];
      } else if (cmd === 'library_input') libraryInput(args.input as LibraryInput);
      else if (cmd === 'remove_entry') {
        const key = args.key as string;
        rows = rows.filter((row) => row.key !== key);
        selectedKeys.delete(key);
        if (focusedKey === key) focusedKey = null;
      } else if (cmd === 'remove_local_game') { /* the mock has no DRM-free registrations */ }
      else if (cmd === 'resolve_local_claim') {
        const transfer = args.transfer as boolean;
        const claim = snapshot.localGames.pendingClaim;
        snapshot.localGames.pendingClaim = null;
        if (claim && !transfer) {
          selectedKeys = new Set([claim.storeKey]);
          focusedKey = claim.storeKey;
        }
        publish();
      } else if (cmd === 'cancel_local_claim') {
        snapshot.localGames.pendingClaim = null;
        publish();
      } else if (cmd === 'select_local_candidate') {
        const fingerprint = args.fingerprint as string;
        const candidate = snapshot.localGames.candidates.find(
          (item) => item.fingerprint === fingerprint && !item.ignored,
        );
        if (!candidate) return undefined;
        const selected = snapshot.localGames.reviewSelected;
        snapshot.localGames.reviewSelected = selected.includes(fingerprint)
          ? selected.filter((item) => item !== fingerprint)
          : [...selected, fingerprint];
        publish();
      } else if (cmd === 'set_local_candidate_executable') {
        const fingerprint = args.fingerprint as string;
        const executable = args.executable as string;
        const candidate = snapshot.localGames.candidates.find(
          (item) => item.fingerprint === fingerprint,
        );
        if (!candidate) throw 'invalidTarget';
        if (
          !candidate.targets.some(
            (target) => target.toLowerCase() === executable.toLowerCase(),
          )
        ) {
          throw 'invalidTarget';
        }
        candidate.selectedTarget = executable;
        candidate.likelyNonGame = candidateLikelyNonGame(candidate);
        publish();
      } else if (cmd === 'review_selected_local_candidates') {
        const add = args.add as boolean;
        const allowRediscovery = args.allowRediscovery as boolean;
        const selected = new Set(snapshot.localGames.reviewSelected);
        if (add) {
          snapshot.localGames.candidates = snapshot.localGames.candidates.filter(
            (candidate) => !selected.has(candidate.fingerprint),
          );
        } else {
          snapshot.localGames.candidates = snapshot.localGames.candidates.flatMap(
            (candidate) => {
              if (!selected.has(candidate.fingerprint)) return [candidate];
              return allowRediscovery
                ? []
                : [{ ...candidate, ignored: true }];
            },
          );
        }
        snapshot.localGames.reviewSelected = snapshot.localGames.reviewSelected.filter(
          (fingerprint) =>
            snapshot.localGames.candidates.some(
              (candidate) => candidate.fingerprint === fingerprint && !candidate.ignored,
            ),
        );
        publish();
      } else if (cmd === 'reset_local_ignores') {
        for (const candidate of snapshot.localGames.candidates) {
          candidate.ignored = false;
        }
        snapshot.localGames.reviewSelected = snapshot.localGames.candidates
          .filter((candidate) => !candidate.likelyNonGame)
          .map((candidate) => candidate.fingerprint);
        publish();
      }
      else if (cmd === 'pick_local_cover') {
        const id = (args.entryKey as string).replace(/^manual:/, '');
        const registration = snapshot.localGames.registrations.find(
          (item) => item.id === id,
        );
        if (!registration) throw 'invalidTarget';
        return 'mock-6.svg';
      } else if (cmd === 'discard_local_cover') {
        // Pending covers live in the editor state; the mock has no file to delete.
      } else if (cmd === 'edit_local_game') {
        const value = args.value as Registration;
        const registration = snapshot.localGames.registrations.find(
          (item) => item.id === value.id,
        );
        if (!registration) throw 'invalidTarget';
        Object.assign(registration, value, {
          customCover: value.customCover ?? null,
        });
        const row = rows.find((item) => item.key === `manual:${registration.id}`);
        if (row) {
          if (!mockGameNodeCovers.has(row.key)) {
            mockGameNodeCovers.set(row.key, row.coverFile ?? null);
          }
          row.gameCoverFile = mockGameNodeCovers.get(row.key) ?? null;
          row.coverFile = registration.customCover ?? row.gameCoverFile;
        }
      } else if (cmd === 'set_local_backup_settings') {
        const retention = Number(args.retention);
        if (![5, 10, 20, 50].includes(retention)) {
          throw { code: 'invalidRetention' };
        }
        const destination = String(args.destination ?? '').trim();
        if (destination.length === 0) {
          throw { code: 'invalidDestination' };
        }
        snapshot.localGames.settings = {
          automatic: args.automatic as boolean,
          destination,
          retention,
        };
      } else if (cmd === 'local_backup_usage') {
        const byInstallation = new Map<string, BackupUsageGame>();
        for (const version of snapshot.localGames.versions) {
          const installation = version.installation;
          const registration = snapshot.localGames.registrations.find(
            (item) => item.id === installation,
          );
          const row = rows.find((item) => item.key === `manual:${installation}`);
          const game = byInstallation.get(installation) ?? {
            entryKey: `manual:${installation}`,
            title: registration?.title ?? row?.title ?? installation,
            coverFile: row?.coverFile ?? null,
            bytes: 0,
            versions: 0,
          };
          game.bytes += version.bytes;
          game.versions += 1;
          byInstallation.set(installation, game);
        }
        const games = [...byInstallation.values()].sort(
          (left, right) => right.bytes - left.bytes,
        );
        return {
          totalBytes: games.reduce((total, game) => total + game.bytes, 0),
          totalVersions: games.reduce((total, game) => total + game.versions, 0),
          games,
        };
      } else if (cmd === 'set_local_backup_retention') {
        const id = (args.entryKey as string).replace(/^manual:/, '');
        const retention = Number(args.retention);
        if (![5, 10, 20, 50].includes(retention)) {
          throw { code: 'invalidRetention' };
        }
        const profile = [...snapshot.localGames.profiles]
          .reverse()
          .find((item) => item.installation === id);
        if (!profile) throw { code: 'invalidTarget' };
        profile.retention = retention;
      } else if (cmd === 'set_local_profile_automatic') {
        const id = (args.id as string).replace(/^manual:/, '');
        const profile = [...snapshot.localGames.profiles]
          .reverse()
          .find((item) => item.installation === id);
        if (!profile) throw { code: 'invalidTarget' };
        profile.automatic = args.automatic as boolean;
      } else if (cmd === 'backup_local_now') {
        const id = (args.id as string).replace(/^manual:/, '');
        const profile = [...snapshot.localGames.profiles]
          .reverse()
          .find((item) => item.installation === id);
        if (!profile) throw { code: 'invalidTarget' };
        const createdAt = Math.floor(Date.now() / 1000);
        snapshot.localGames.versions = [
          {
            installation: id,
            profile: profile.id,
            destination: `D:\\Backups\\${id}`,
            id: `manual-backup-${createdAt}`,
            label: new Date(createdAt * 1000).toISOString().slice(0, 19).replace('T', ' '),
            bytes: 7_100_000,
            createdAt,
            reason: 'manual',
            playSeconds: null,
          },
          ...snapshot.localGames.versions,
        ];
        snapshot.localGames.historyBytes = snapshot.localGames.versions.reduce(
          (total, version) => total + version.bytes,
          0,
        );
        snapshot.localGames.historyCount = snapshot.localGames.versions.length;
      } else if (cmd === 'load_local_history' || cmd === 'preview_local_restore' || cmd === 'cancel_local_restore' || cmd === 'confirm_local_restore') {
        // The fixture history is already loaded; restore previews are host-owned.
      }
      else if (cmd === 'set_sync_enabled') snapshot.sync.enabled = args.enabled as boolean;
      else if (cmd === 'dismiss_sync_explanation') snapshot.sync.explained = true;
      else if (cmd === 'sync_now') {
        const signedIn = typeof snapshot.auth === 'object' && 'signedIn' in snapshot.auth;
        if (!snapshot.sync.enabled || !signedIn) throw 'syncUnavailable';
        if (snapshot.sync.running) return undefined;
        snapshot.sync.running = true;
        publish();
        setTimeout(() => {
          const accepted = rows.filter((row) => (row.playtime?.seconds ?? 0) > 0);
          snapshot.sync.summary = {
            completedAt: Math.floor(Date.now() / 1000),
            acceptedGames: accepted.length,
            stores: new Set(accepted.map((row) => row.provider)).size,
            held: 0,
          };
          snapshot.sync.running = false;
          publish();
        }, 350);
      } else if (cmd === 'check_local_watch') {
        const watch = snapshot.localGames.watches.find(
          (item) => item.id === (args.id as string),
        );
        if (!watch || !watch.enabled || watch.state === 'unavailable') return undefined;
        watch.state = 'checking';
        publish();
        setTimeout(() => {
          watch.state = 'idle';
          watch.lastCheckedAt = Math.floor(Date.now() / 1000);
          publish();
        }, 250);
      }
      else if (cmd === 'set_playtime_source') snapshot.playtimeSource = args.source as Snapshot['playtimeSource'];
      else if (cmd === 'set_app_preference') {
        snapshot.preferences = applyMockAppPreference(
          snapshot.preferences,
          args.patch as AppPreferencePatch,
        );
      }
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
        const sort = (args.sort as string | undefined) ?? 'mostRecent';
        if (!key.startsWith('steam:')) return null;
        return mockAchievementList(filter, sort);
      } else if (cmd === 'open_achievements_page') {
        const key = (args.entryKey as string | undefined) ?? '';
        const appId = key.startsWith('steam:') ? key.slice('steam:'.length) : '';
        if (!/^\d+$/.test(appId)) throw { code: 'unsupported' };
        console.info('[UI mock] openAchievementsPage', appId);
        return undefined;
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
      } else if (cmd === 'open_tools') {
        openMockTools(args.entryKey as string | null | undefined);
      } else if (cmd === 'open_tools_for_selected') {
        openMockTools();
      } else if (cmd === 'close_tools') {
        snapshot.tools.modal = null;
      } else if (cmd === 'select_tool_channel') {
        const modal = snapshot.tools.modal;
        const channel = args.channel as ToolChannel;
        if (modal) {
          modal.selectedChannel = channel;
          modal.availableVersions = MOCK_TOOL_RELEASES.filter(
            (release) => release.channel === channel,
          );
          modal.selectedVersion = modal.availableVersions[0]?.version ?? null;
          modal.operation = null;
        }
      } else if (cmd === 'refresh_tool_versions') {
        const modal = snapshot.tools.modal;
        if (modal) {
          modal.availableVersions = MOCK_TOOL_RELEASES.filter(
            (release) => release.channel === modal.selectedChannel,
          );
          modal.selectedVersion =
            modal.availableVersions[0]?.version ?? modal.selectedVersion;
        }
      } else if (cmd === 'select_tool_version') {
        const modal = snapshot.tools.modal;
        if (modal) modal.selectedVersion = args.version as string;
      } else if (cmd === 'select_tool_target_exe') {
        const modal = snapshot.tools.modal;
        if (modal?.inspection) {
          modal.inspection.selectedExe = args.relativeExe as string;
        }
      } else if (cmd === 'pick_tool_target_exe') {
        // Parity with the host's executable picker: a chosen file is selected through the same
        // command the candidate list uses; cancelling keeps the target and a failure reports it.
        const modal = snapshot.tools.modal;
        if (options.toolPick === 'cancel') {
          // The native dialog was dismissed.
        } else if (options.toolPick === 'fail') {
          if (modal) {
            modal.operation = {
              failed: {
                code: 'pickerFailed',
                message: 'The executable picker could not open.',
              },
            };
          }
        } else if (options.toolPick === 'outside') {
          if (modal) {
            modal.operation = {
              failed: {
                code: 'targetOutsideInstallDir',
                message: 'Target executable path escapes the game directory',
              },
            };
          }
        } else if (modal?.inspection) {
          modal.inspection.selectedExe = 'bin/x64/picked.exe';
          if (
            modal.operation &&
            typeof modal.operation === 'object' &&
            'failed' in modal.operation
          ) {
            modal.operation = null;
          }
        }
      } else if (cmd === 'select_tool_proxy') {
        const modal = snapshot.tools.modal;
        if (modal) modal.selectedProxy = args.proxy as ToolModalView['selectedProxy'];
      } else if (cmd === 'set_tool_anticheat_confirmed') {
        const modal = snapshot.tools.modal;
        if (modal) modal.anticheatConfirmed = args.confirmed as boolean;
      } else if (cmd === 'set_tool_reset_config') {
        // The reset choice is not part of the view; the host keeps it for the install request.
      } else if (cmd === 'install_or_update_tool') {
        const modal = snapshot.tools.modal;
        if (modal) {
          const hasWarning = (modal.inspection?.anticheat.detected.length ?? 0) > 0;
          if (hasWarning && !modal.anticheatConfirmed) {
            modal.operation = {
              failed: {
                message: 'Anti-cheat warning must be confirmed before installing',
              },
            };
          } else {
            installedMockTool(modal);
          }
        }
      } else if (cmd === 'uninstall_tool') {
        const modal = snapshot.tools.modal;
        if (modal) {
          modal.tools = [];
          modal.operation = null;
          delete snapshot.tools.installedByEntry[modal.entryKey];
          refreshMockToolCacheInUse();
        }
      } else if (cmd === 'open_tool_source') {
        console.info('[UI mock] openToolSource', args.url);
      } else if (cmd === 'clear_tool_cache') {
        snapshot.tools.totalCacheBytes = 0;
        snapshot.tools.totalCacheArchives = 0;
        snapshot.tools.achievementIconCacheBytes = 0;
        snapshot.tools.cacheEntries = [];
        for (const modal of [snapshot.tools.modal]) {
          for (const release of modal?.availableVersions ?? []) {
            release.cachedPath = null;
          }
        }
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

/**
 * Maps the documented `VITE_UI_MOCK=1` query parameters to host options. Kept
 * pure so the scenario table in docs/development.md can be tested without
 * reloading the module.
 */
export function mockHostOptionsFromSearch(
  search: string,
): NonNullable<Parameters<typeof createMockHost>[0]> {
  const requestedParams = new URLSearchParams(search);
  const requestedSteamScenario = requestedParams.get('steam');
  const requestedEpicScenario = requestedParams.get('epic');
  const requestedCatalogScenario = requestedParams.get('catalog');
  return {
    signedOut: requestedParams.get('scenario') === 'signed-out',
    firstRun: requestedParams.get('scenario') === 'first-run',
    locale: requestedParams.get('locale') === 'pt-BR' ? 'pt-BR' : 'en',
    storeAccount: isMockSteamScenario(requestedSteamScenario)
      ? requestedSteamScenario
      : undefined,
    epicAccount: isMockSteamScenario(requestedEpicScenario)
      ? requestedEpicScenario
      : undefined,
    // `?watches=unavailable` seeds the two Settings › Jogos DRM-free folder rows.
    watches:
      requestedParams.get('watches') === 'unavailable' ? 'unavailable' : undefined,
    largeLibrary: requestedParams.get('library') === 'large',
    variantLibrary: requestedParams.get('library') === 'variants',
    emptyLibrary: requestedParams.get('library') === 'empty',
    noInstalledLibrary: requestedParams.get('library') === 'no-installed',
    discoveries: requestedParams.get('discoveries') === '1',
    claim: requestedParams.get('claim') === '1',
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
    portsScenario:
      requestedParams.get('ports') === 'unavailable' ? 'unavailable' : 'default',
    catalogScenario: isMockCatalogScenario(requestedCatalogScenario)
      ? requestedCatalogScenario
      : undefined,
    failMutations: requestedParams.get('mutations') === 'fail',
    // `?scenario=signed-out&signin=pending` keeps the browser round trip open; `reopen=fail`
    // makes Open again report the localized error without ending the attempt.
    signInPending: requestedParams.get('signin') === 'pending',
    signInReopenFails: requestedParams.get('reopen') === 'fail',
    // First successful sign-in on this PC: the sync explanation is pending. Keep on dismisses it;
    // Turn off disables sync and then dismisses it in the same action.
    syncFirstRun: requestedParams.get('sync') === 'first',
    // `?toolpick=cancel`, `?toolpick=fail` or `?toolpick=outside` exercises the Browse… failure paths.
    toolPick:
      requestedParams.get('toolpick') === 'cancel'
        ? 'cancel'
        : requestedParams.get('toolpick') === 'fail'
          ? 'fail'
          : requestedParams.get('toolpick') === 'outside'
            ? 'outside'
            : 'accept',
    // The browser mock shows the batch one game at a time; tests pass 0 for a synchronous run.
    addBatchDelayMs: 650,
  };
}

export const mockHost = createMockHost(mockHostOptionsFromSearch(window.location.search));
