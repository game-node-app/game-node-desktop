/** An isolated, in-memory host for interactive UI development. */
import {
  createDefaultFilters,
  createDefaultSnapshot,
  getMatchedGameId,
  type AuthInput,
  type LibraryFilters,
  type LibraryInput,
  type LibraryView,
  type ProviderCount,
  type Row,
  type Snapshot,
  type StorageMethod,
  type StoreAccountState,
  type StoreAccountView,
} from '../api/types';
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

type Listener = (snapshot: Snapshot) => void;
type ProgressListener = (progress: PortProgress) => void;

export type MockSteamScenario =
  | 'notConnected'
  | 'checking'
  | 'signingIn'
  | 'connected'
  | 'expired'
  | 'unreachable'
  | 'credentialError'
  | 'persistFailed';

const titles = ['Aster Vale', 'Beyond the Pines', 'Clockwork Harbor', 'Distant Skies', 'Emberfall', 'Forgotten Atlas', 'Glass Garden', 'Hollow Mountain', 'Infinite Coast', 'Juniper Station', 'Kingdom of Rain', 'Lunar Circuit'];

/**
 * The mock library mixes installed, owned-only and family-shared rows so every
 * indicator and count can be exercised from `VITE_UI_MOCK=1`.
 */
const NOW = Math.floor(Date.now() / 1000);

const allRows: Row[] = titles.map((title, index) => {
  const steam = index % 2 === 0;
  const installed = steam ? index === 0 || index === 2 || index === 8 : true;
  const familyShared = index === 4;
  const owned = steam && !familyShared;
  return {
    key: `${steam ? 'steam' : 'gog'}:${index + 1}`,
    title, detectedName: title, matchedName: index === 2 ? null : title,
    provider: steam ? 'steam' : 'gog', providerName: steam ? 'Steam' : 'GOG',
    installed, owned, familyShared,
    running: false, status: index === 2 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
    coverFile: `mock-${index}.svg`,
    playtime: {
      seconds: index * 3720,
      source: 'store',
      lastPlayedAt: index === 0 ? null : NOW - index * 86_400,
    },
    releaseYear: index === 2 ? null : 1992 + index * 2,
    addedAt: NOW - index * 3 * 86_400,
    isFavorite: index === 1 || index === 3,
  };
});

const DAY = 86_400;
const YEAR = 365 * DAY;

function within(at: number | null | undefined, now: number, window: number): boolean {
  return at != null && at >= now - window;
}

/** The same facet rules as `crates/core/src/library/query.rs`, in memory. */
function matchesFilters(row: Row, filters: LibraryFilters, now: number): boolean {
  const availability = filters.availability;
  const anyAvailability = availability.installed || availability.owned || availability.family;
  if (
    anyAvailability &&
    !(
      (availability.installed && row.installed) ||
      (availability.owned && row.owned) ||
      (availability.family && row.familyShared)
    )
  ) return false;
  if (filters.runningOnly && !row.running) return false;
  switch (filters.matchState) {
    case 'matched':
      if (typeof row.status !== 'object' || !('matched' in row.status)) return false;
      break;
    case 'ambiguous':
      if (row.status !== 'ambiguous') return false;
      break;
    case 'unmatched':
      if (row.status !== 'unmatched') return false;
      break;
    case 'noGame':
      if (row.status !== 'noGame') return false;
      break;
    default:
      break;
  }
  const seconds = row.playtime.seconds ?? null;
  switch (filters.playtime) {
    case 'unplayed':
      if (seconds !== 0) return false;
      break;
    case 'played':
      if (seconds == null || seconds <= 0) return false;
      break;
    case 'atLeast10h':
      if (seconds == null || seconds < 10 * 3600) return false;
      break;
    case 'atLeast100h':
      if (seconds == null || seconds < 100 * 3600) return false;
      break;
    default:
      break;
  }
  const lastPlayed = row.playtime.lastPlayedAt ?? null;
  switch (filters.lastPlayed) {
    case 'last7Days':
      if (!within(lastPlayed, now, 7 * DAY)) return false;
      break;
    case 'last30Days':
      if (!within(lastPlayed, now, 30 * DAY)) return false;
      break;
    case 'lastYear':
      if (!within(lastPlayed, now, YEAR)) return false;
      break;
    default:
      break;
  }
  const release = filters.release;
  const year = row.releaseYear ?? null;
  if (typeof release === 'object' && 'decade' in release) {
    if (year == null || year < release.decade || year >= release.decade + 10) return false;
  } else if (release === 'earlier') {
    if (year == null || year >= 1980) return false;
  } else if (release === 'unknown' && year != null) {
    return false;
  }
  switch (filters.added) {
    case 'last7Days':
      if (!within(row.addedAt, now, 7 * DAY)) return false;
      break;
    case 'last30Days':
      if (!within(row.addedAt, now, 30 * DAY)) return false;
      break;
    case 'last90Days':
      if (!within(row.addedAt, now, 90 * DAY)) return false;
      break;
    case 'lastYear':
      if (!within(row.addedAt, now, YEAR)) return false;
      break;
    default:
      break;
  }
  if (filters.favoritesOnly && !row.isFavorite) return false;
  return true;
}

function compareOptional(
  a: number | null | undefined,
  b: number | null | undefined,
  descending: boolean,
): number {
  const left = a ?? null;
  const right = b ?? null;
  if (left == null && right == null) return 0;
  if (left == null) return 1;
  if (right == null) return -1;
  return descending ? right - left : left - right;
}

function compareRows(a: Row, b: Row, sort: { key: LibraryView['layout']['sort']['key']; direction: 'ascending' | 'descending' }): number {
  const descending = sort.direction === 'descending';
  const flip = (order: number) => (descending ? -order : order);
  switch (sort.key) {
    case 'provider':
      return flip(a.providerName.localeCompare(b.providerName));
    case 'matchStatus': {
      const rank = (row: Row) =>
        typeof row.status === 'object' && 'matched' in row.status
          ? 0
          : row.status === 'ambiguous'
            ? 1
            : row.status === 'unmatched'
              ? 2
              : 3;
      return flip(rank(a) - rank(b));
    }
    case 'releaseYear':
      return compareOptional(a.releaseYear, b.releaseYear, descending);
    case 'playtime':
      return compareOptional(a.playtime.seconds, b.playtime.seconds, descending);
    case 'lastPlayed':
      return compareOptional(a.playtime.lastPlayedAt, b.playtime.lastPlayedAt, descending);
    case 'dateAdded':
      return compareOptional(a.addedAt, b.addedAt, descending);
    default:
      return flip(a.title.localeCompare(b.title));
  }
}

function summarizeProvider(filtered: Row[], raw: Row[], id: string, name: string): ProviderCount {
  const providerRows = filtered.filter((row) => row.provider === id);
  const rawRows = raw.filter((row) => row.provider === id);
  return {
    id,
    name,
    count: providerRows.length,
    installedCount: rawRows.filter((row) => row.installed).length,
    ownedCount: rawRows.filter((row) => row.owned).length,
    familyCount: rawRows.filter((row) => row.familyShared && !row.owned).length,
  };
}

function providerCounts(filtered: Row[], raw: Row[]): ProviderCount[] {
  return [
    summarizeProvider(filtered, raw, 'steam', 'Steam'),
    summarizeProvider(filtered, raw, 'gog', 'GOG'),
  ];
}

function createLibrary(source: Row[]): LibraryView {
  return {
    generation: 1, layout: { view: 'grid', sort: { key: 'title', direction: 'ascending' }, filter: 'all', filters: createDefaultFilters(), sidebarWidth: 248, sidebarCollapsed: false, detailWidth: 320, detailOpen: false },
    search: '', access: 'verified', total: source.length, installedTotal: source.filter((row) => row.installed).length,
    providers: providerCounts(source, source),
    rows: structuredClone(source), scanning: false, failedProviders: [], selected: [], focused: null,
    actions: { selected: 0, play: { disabled: 'nothingSelected' }, install: { disabled: 'nothingSelected' }, matchGame: { disabled: 'nothingSelected' }, addToGamenode: { disabled: 'nothingSelected' }, changeMatch: false },
  };
}

/**
 * Optional large-library fixture (`?library=large`): 320 owned Steam games, exactly 18 installed,
 * and a few family-shared games, so virtualization, counts and the Installed filter can be checked
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
      familyShared: false,
      running: false,
      status: index % 5 === 0 ? 'unmatched' : { matched: { gameId: index + 1, source: 'automatic' } },
      coverFile: `mock-${index % 12}.svg`,
      playtime: { seconds: installed ? index * 600 : null, source: 'store', lastPlayedAt: installed ? NOW - index * 3600 : null },
      releaseYear: 2000 + (index % 25),
      addedAt: NOW - index * DAY,
      isFavorite: false,
    });
  }
  for (let index = 0; index < 8; index += 1) {
    rows.push({
      key: `steam:${5000 + index}`,
      title: `Family Shelf ${index + 1}`,
      detectedName: `Family Shelf ${index + 1}`,
      matchedName: null,
      provider: 'steam',
      providerName: 'Steam',
      installed: false,
      owned: false,
      familyShared: true,
      running: false,
      status: 'unmatched',
      coverFile: `mock-${index % 12}.svg`,
      playtime: { seconds: null, source: 'store', lastPlayedAt: null },
      releaseYear: null,
      addedAt: NOW - index * DAY,
      isFavorite: false,
    });
  }
  return rows;
}

function installedOnlyRows(source: Row[]): Row[] {
  return source
    .map((row) => row.provider === 'steam' ? { ...row, owned: false, familyShared: false } : { ...row })
    .filter((row) => row.installed || row.owned || row.familyShared);
}

function makeStoreAccount(
  scenario: MockSteamScenario,
  counts: Pick<StoreAccountView, 'ownedCount' | 'familyCount' | 'installedCount'>,
  storage: StorageMethod,
): StoreAccountView {
  const disconnected = scenario === 'notConnected';
  const signingIn = scenario === 'signingIn';
  const state: StoreAccountState = scenario === 'persistFailed' ? 'connected' : scenario;
  const identity = scenario === 'notConnected' || signingIn
    ? null
    : { accountId: '76561198000000000', accountName: 'cassi' };
  const catalogError =
    scenario === 'expired' ? 'expired' : scenario === 'unreachable' ? 'unreachable' : null;

  return {
    provider: 'steam',
    state,
    accountId: identity?.accountId ?? null,
    accountName: identity?.accountName ?? null,
    ownedCount: disconnected ? 0 : counts.ownedCount,
    familyCount: disconnected ? 0 : counts.familyCount,
    installedCount: counts.installedCount,
    lastCheckedAt: disconnected || signingIn ? null : Math.floor(Date.now() / 1000) - 120,
    lastSyncedAt: state === 'connected' ? Math.floor(Date.now() / 1000) : null,
    storage,
    persistFailed: scenario === 'persistFailed',
    catalogError,
    familyError: null,
    canSignIn: scenario === 'notConnected' || scenario === 'expired' || scenario === 'credentialError',
    canCancelSignIn: signingIn,
    canFocusSignIn: signingIn,
    canSignOut: scenario === 'connected' || scenario === 'expired' || scenario === 'unreachable' || scenario === 'persistFailed',
    canCheck: scenario === 'connected' || scenario === 'unreachable' || scenario === 'credentialError' || scenario === 'persistFailed',
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

export function createMockHost(options: { signedOut?: boolean; locale?: string; storeAccount?: MockSteamScenario; steamStorage?: StorageMethod; largeLibrary?: boolean; platform?: PortPlatform; portsScenario?: 'default' | 'unavailable' } = {}) {
  const listeners = new Set<Listener>();
  const progressListeners = new Set<ProgressListener>();
  const snapshot = createDefaultSnapshot();
  const ports = createMockPortsView(options.portsScenario ?? 'default');
  snapshot.ports = ports;
  const baseRows = options.largeLibrary ? createLargeLibraryRows() : allRows;
  let rows = structuredClone(baseRows);
  let steamScenario: MockSteamScenario = options.storeAccount ?? 'connected';
  const steamStorage: StorageMethod = options.steamStorage ?? 'secretService';
  let steamGeneration = 0;
  let previousSteamScenario: MockSteamScenario =
    steamScenario === 'signingIn' ? 'notConnected' : steamScenario;

  function steamCounts() {
    return {
      ownedCount: rows.filter((row) => row.provider === 'steam' && row.owned).length,
      familyCount: rows.filter((row) => row.provider === 'steam' && row.familyShared && !row.owned).length,
      installedCount: rows.filter((row) => row.provider === 'steam' && row.installed).length,
    };
  }

  function applySteamScenario(scenario: MockSteamScenario) {
    steamScenario = scenario;
    snapshot.storeAccounts = [makeStoreAccount(scenario, steamCounts(), steamStorage)];
  }

  function clearSteamCatalog() {
    rows = rows
      .map((row) => row.provider === 'steam' ? { ...row, owned: false, familyShared: false } : row)
      .filter((row) => row.installed || row.owned || row.familyShared);
  }

  function scheduleSteam(delay: number, action: () => void) {
    const generation = ++steamGeneration;
    setTimeout(() => {
      if (generation !== steamGeneration) return;
      action();
    }, delay);
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

  function signIn() {
    snapshot.auth = { signedIn: { user: { id: 'mock-user', username: 'UI Developer', email: 'ui@example.invalid' } } };
    rows = steamScenario === 'notConnected' ? installedOnlyRows(baseRows) : structuredClone(baseRows);
    snapshot.library = createLibrary(rows);
  }
  if (!options.signedOut) signIn();
  applySteamScenario(steamScenario);
  const copy = () => structuredClone(snapshot);
  function updateLibrary() {
    const lib = snapshot.library;
    if (!lib) return;
    const filter = lib.layout.filter;
    const sort = lib.layout.sort;
    const filters = lib.layout.filters;
    const visible = rows.filter((row) => row.installed || row.owned || row.familyShared);
    const byFacets = visible
      .filter((row) => matchesFilters(row, filters, NOW))
      .filter((row) => row.title.toLowerCase().includes(lib.search.toLowerCase()));
    lib.total = visible.length;
    lib.installedTotal = visible.filter((row) => row.installed).length;
    lib.providers = providerCounts(byFacets, visible);
    lib.rows = byFacets
      .filter((row) => filter === 'all' || row.provider === filter.provider)
      .sort((a, b) => compareRows(a, b, sort));
    const availability = filters.availability;
    const installedOnly =
      availability.installed && !availability.owned && !availability.family;
    if (visible.length === 0) {
      lib.empty = { noGames: { stores: ['Steam', 'GOG'] } };
    } else if (installedOnly && lib.installedTotal === 0) {
      lib.empty = { noInstalled: { provider: filter === 'all' ? null : filter.provider } };
    } else if (lib.rows.length === 0) {
      lib.empty = 'nothingMatches';
    } else {
      lib.empty = null;
    }
    const selected = rows.filter((row) => lib.selected.includes(row.key));
    const one = selected.length === 1 ? selected[0] : undefined;
    const disabled = { disabled: 'nothingSelected' as const };
    lib.actions = { selected: selected.length, play: one ? 'available' : disabled, install: disabled,
      matchGame: one ? 'available' : disabled, addToGamenode: selected.some((row) => typeof row.status === 'object') ? 'available' : disabled, changeMatch: !!one };
    lib.detail = one ? { row: one, running: false, installDir: `/games/${one.title}`, releaseYear: one.releaseYear ?? 2022, hasArtwork: true, membership: 'notInLibrary' } : null;
  }
  function publish() { updateLibrary(); const next = copy(); listeners.forEach((listener) => listener(next)); }
  function libraryInput(input: LibraryInput) {
    const lib = snapshot.library;
    if (!lib) return;
    if (typeof input === 'object') {
      if ('setSearch' in input) lib.search = input.setSearch;
      else if ('setView' in input) lib.layout.view = input.setView;
      else if ('setFilter' in input) lib.layout.filter = input.setFilter;
      else if ('setFilters' in input) lib.layout.filters = input.setFilters;
      else if ('setFavorite' in input) {
        const row = rows.find((item) => item.key === input.setFavorite.key);
        const gameId = row ? getMatchedGameId(row.status) : null;
        if (gameId != null) rows = rows.map((item) => getMatchedGameId(item.status) === gameId ? { ...item, isFavorite: input.setFavorite.favorite } : item);
      }
      else if ('setSort' in input) lib.layout.sort = input.setSort;
      else if ('setDetailOpen' in input) lib.layout.detailOpen = input.setDetailOpen;
      else if ('setDetailWidth' in input) lib.layout.detailWidth = input.setDetailWidth;
      else if ('setSidebarWidth' in input) lib.layout.sidebarWidth = input.setSidebarWidth;
      else if ('setSidebarCollapsed' in input) lib.layout.sidebarCollapsed = input.setSidebarCollapsed;
      else if ('select' in input) {
        const { key, mode } = input.select;
        lib.selected = mode === 'toggle' ? (lib.selected.includes(key) ? lib.selected.filter((item) => item !== key) : [...lib.selected, key]) : [key];
        lib.focused = key;
      } else if ('move' in input && lib.rows.length) {
        const index = Math.max(0, lib.rows.findIndex((row) => row.key === lib.focused));
        const direction = input.move.direction;
        const next = direction === 'home' ? 0 : direction === 'end' ? lib.rows.length - 1 : Math.max(0, Math.min(lib.rows.length - 1, index + (direction === 'left' || direction === 'up' ? -1 : 1)));
        lib.focused = lib.rows[next].key; lib.selected = [lib.focused];
      } else if ('setMatchQuery' in input && lib.matchDialog) lib.matchDialog.query = input.setMatchQuery;
      else if ('pickMatch' in input && lib.matchDialog) {
        const row = rows.find((item) => item.key === lib.matchDialog?.entryKey);
        if (row) { row.status = { matched: { gameId: input.pickMatch.gameId, source: 'chosen' } }; row.matchedName = row.title; }
        lib.matchDialog = null;
      } else if ('setAddStatus' in input && lib.addDialog) lib.addDialog.status = input.setAddStatus;
      else if ('toggleAddCollection' in input && lib.addDialog && typeof lib.addDialog.collections === 'object' && 'loaded' in lib.addDialog.collections) {
        const collection = lib.addDialog.collections.loaded.find((item) => item.id === input.toggleAddCollection);
        if (collection) collection.picked = !collection.picked;
      }
    } else if (input === 'openDetails') lib.layout.detailOpen = true;
    else if (input === 'closeMatch') lib.matchDialog = null;
    else if (input === 'openMatch') {
      const row = rows.find((item) => item.key === lib.focused);
      if (row) lib.matchDialog = { entryKey: row.key, detectedName: row.detectedName, providerName: row.providerName, query: row.title, searching: false, candidates: [{ gameId: 100, name: row.title, current: false }], results: [], noResults: false, current: row.status, canClear: typeof row.status === 'object' };
    } else if (input === 'searchMatch' && lib.matchDialog) {
      lib.matchDialog.results = [{ gameId: 100, name: lib.matchDialog.query, current: false }];
    } else if (input === 'openAdd') {
      lib.addDialog = { targets: rows.filter((row) => lib.selected.includes(row.key)).map((row) => ({ entryKey: row.key, title: row.title, matched: typeof row.status === 'object' })), editingExisting: false, status: 'playing', appliedStatus: 'playing', collections: { loaded: [{ id: 'favorites', name: 'Favorites', picked: false }] }, canConfirm: true };
    } else if (input === 'closeAdd' || input === 'confirmAdd') lib.addDialog = null;
  }
  return {
    subscribe(listener: Listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
    subscribePortProgress(listener: ProgressListener) {
      progressListeners.add(listener);
      return () => { progressListeners.delete(listener); };
    },
    async invoke(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (cmd === 'get_snapshot') return copy();
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
        if (input === 'signOut') { snapshot.auth = 'signedOut'; snapshot.library = null; }
        else if (typeof input === 'object' && 'startSignIn' in input) signIn();
      } else if (cmd === 'library_input') libraryInput(args.input as LibraryInput);
      else if (cmd === 'remove_entry') {
        const key = args.key as string;
        rows = rows.filter((row) => row.key !== key);
        const lib = snapshot.library;
        if (lib) {
          lib.selected = lib.selected.filter((item) => item !== key);
          if (lib.focused === key) lib.focused = null;
        }
      } else if (cmd === 'remove_local_game') { /* the mock has no DRM-free registrations */ }
      else if (cmd === 'set_sync_enabled') snapshot.sync.enabled = args.enabled as boolean;
      else if (cmd === 'dismiss_sync_explanation') snapshot.sync.explained = true;
      else if (cmd === 'set_playtime_source') snapshot.playtimeSource = args.source as Snapshot['playtimeSource'];
      else if (cmd === 'steam_sign_in') {
        previousSteamScenario = steamScenario === 'signingIn' ? 'notConnected' : steamScenario;
        applySteamScenario('signingIn');
        scheduleSteam(1500, () => {
          rows = structuredClone(baseRows);
          applySteamScenario('connected');
          publish();
        });
      } else if (cmd === 'steam_cancel_sign_in') {
        steamGeneration += 1;
        applySteamScenario(steamScenario === 'signingIn' ? previousSteamScenario : steamScenario);
      } else if (cmd === 'steam_focus_sign_in') { /* the mock has no separate window to focus */ }
      else if (cmd === 'steam_sign_out') {
        steamGeneration += 1;
        clearSteamCatalog();
        applySteamScenario('notConnected');
      } else if (cmd === 'steam_check') {
        applySteamScenario('checking');
        scheduleSteam(900, () => {
          rows = structuredClone(baseRows);
          applySteamScenario('connected');
          publish();
        });
      } else if (cmd === 'rescan_library' || cmd === 'update_window_geometry' || cmd === 'dismiss_launch') { /* no external effects */ }
      else if (cmd === 'ports_open') {
        if (ports.catalog.state !== 'ready' && ports.catalog.state !== 'refreshing') {
          refreshPortCatalog();
        }
      } else if (cmd === 'ports_refresh') {
        refreshPortCatalog();
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
    || value === 'credentialError' || value === 'persistFailed';
}

/** `?steam=expired` (or any `MockSteamScenario`) selects a store-account fixture. */
const requestedSteamScenario = new URLSearchParams(window.location.search).get('steam');
const requestedParams = new URLSearchParams(window.location.search);

export const mockHost = createMockHost({
  signedOut: requestedParams.get('scenario') === 'signed-out',
  locale: requestedParams.get('locale') === 'pt-BR' ? 'pt-BR' : 'en',
  storeAccount: isMockSteamScenario(requestedSteamScenario) ? requestedSteamScenario : undefined,
  largeLibrary: requestedParams.get('library') === 'large',
  // PC ports are Windows-only in this release. The mock is a Windows signed-in build by
  // default so the destination is visible in a browser on any OS; `?platform=linux`
  // exercises the hidden case.
  platform: requestedParams.get('platform') === 'linux' ? 'linux' : 'windows',
  portsScenario: requestedParams.get('ports') === 'unavailable' ? 'unavailable' : 'default',
});
