import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
} from '../../api';
import {
  portProgressQueryKey,
  type PortDetailView,
  type PortEntryView,
  type PortJobView,
  type PortSetupSlot,
  type PortsView as PortsViewPayload,
} from '../../api/ports';
import { I18nProvider } from '../../i18n';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { PortsView } from './PortsView';

vi.mock('./setup', async (importOriginal) => {
  const actual = await importOriginal<typeof import('./setup')>();
  return {
    ...actual,
    openPortSetup: vi.fn(),
  };
});

import { openPortSetup } from './setup';

const NOW = 1_700_000_000;

function slot(overrides: Partial<PortSetupSlot> & { id: string }): PortSetupSlot {
  return {
    label: 'A file',
    prompt: null,
    kind: 'file',
    required: true,
    extensions: [],
    contains: [],
    acceptedLabels: [],
    destination: null,
    root: null,
    argument: null,
    packageOption: null,
    ...overrides,
  };
}

function entry(overrides: Partial<PortEntryView> & { catalogId: string }): PortEntryView {
  return {
    slug: overrides.catalogId,
    name: overrides.catalogId,
    games: ['A game'],
    consoles: ['PlayStation 4'],
    supportedOS: ['windows'],
    verified: true,
    prerelease: false,
    version: '1.0.0',
    guided: false,
    withdrawn: false,
    hasWindowsAsset: true,
    noAssetReason: null,
    installable: true,
    installBlockReason: null,
    art: { capsule: true, hero: true },
    project: null,
    source: null,
    license: null,
    requirements: [],
    installedRegistrationKey: null,
    ...overrides,
  };
}

const packageOption = {
  extractorId: 'ps4-pkg',
  toolName: 'Mock PKG Extractor',
  toolSourceUrl: null,
  toolLicense: 'GPL-3.0',
  titleId: 'CUSA10101',
  updateVersion: '01.04',
  requiresUpdate: true,
};

function makeEntries(): PortEntryView[] {
  return [
    entry({
      catalogId: 'qcat_aster',
      name: 'Aster Vale: Revival',
      games: ['Vale of Ashes'],
      consoles: ['PlayStation 4'],
      version: '1.4.2',
      guided: true,
      license: 'GPL-3.0-or-later',
      project: { label: 'Aster Vale Revival', url: 'https://example.invalid/aster' },
      requirements: [
        slot({
          id: 'game-folder',
          label: 'Aster Vale (CUSA10101) game folder',
          prompt: 'Choose the folder from your own disc.',
          kind: 'folder',
          contains: ['eboot.bin', 'sce_sys/param.sfo'],
          acceptedLabels: ['CUSA10101 (01.04)'],
          destination: 'CUSA10101',
          root: 'game',
          packageOption,
        }),
        slot({
          id: 'settings-backup',
          label: 'Settings backup',
          required: false,
          extensions: ['.zip'],
          argument: { kind: 'flag', name: '--settings' },
        }),
      ],
    }),
    entry({
      catalogId: 'qcat_beyond',
      name: 'Beyond the Pines',
      games: ['Beyond the Pines'],
      consoles: ['PlayStation 3'],
      version: '0.9.1',
    }),
    entry({
      catalogId: 'qcat_clockwork',
      name: 'Clockwork Harbor',
      games: ['Clockwork Harbor'],
      verified: false,
      prerelease: true,
      version: '2.0.0-alpha.3',
      installable: false,
      installBlockReason: 'noVerifiedRelease',
    }),
    entry({
      catalogId: 'qcat_distant',
      name: 'Distant Skies: Recompiled',
      games: ['Distant Skies'],
      consoles: ['PlayStation 3'],
      withdrawn: true,
      installable: false,
      installBlockReason: 'withdrawn',
    }),
    entry({
      catalogId: 'qcat_emberfall',
      name: 'Emberfall: Iron Trail',
      games: ['Emberfall'],
      version: '1.0.5',
      installedRegistrationKey: 'drmfree:emberfall',
    }),
    entry({
      catalogId: 'qcat_glass',
      name: 'Glass Garden: Enhanced',
      games: ['Glass Garden'],
      consoles: ['PC'],
      hasWindowsAsset: false,
      noAssetReason: 'installerOnly',
      installable: false,
      installBlockReason: 'installerOnly',
      art: { capsule: false, hero: false },
    }),
  ];
}

function makePorts(overrides: Partial<PortsViewPayload> = {}): PortsViewPayload {
  const entries = makeEntries();
  return {
    catalog: {
      state: 'ready',
      fetchedAt: NOW - 3600,
      entryCount: entries.length,
      error: null,
    },
    entries,
    provenance: {
      'drmfree:emberfall': {
        registrationKey: 'drmfree:emberfall',
        catalogId: 'qcat_emberfall',
        slug: 'emberfall-iron-trail',
        portName: 'Emberfall: Iron Trail',
        installedVersion: '1.0.4',
        installFolder: 'D:\\Ports\\Emberfall',
        jobId: null,
        installedAt: NOW - 5000,
      },
    },
    jobs: [],
    unseenCount: 0,
    preferences: {
      view: 'grid',
      filters: { consoles: [], guided: false, verified: false, installed: false },
      lastParentFolder: null,
    },
    ...overrides,
  };
}

function releaseFor(entry: PortEntryView, version: string): NonNullable<PortDetailView['selectedRelease']> {
  return {
    version,
    releasedAt: '2026-09-18',
    verified: entry.verified,
    withdrawn: entry.withdrawn,
    assetName: entry.hasWindowsAsset ? `${entry.slug}-windows.zip` : null,
    assetFormat: entry.hasWindowsAsset ? 'zip' : null,
    sizeBytes: entry.hasWindowsAsset ? 1_500_000_000 : null,
    checksumVerified: entry.verified,
    requiresChecksumConfirmation: !entry.verified && entry.hasWindowsAsset,
    malwareFlag: null,
  };
}

function detailForEntry(catalogId: string): PortDetailView {
  const entry = makeEntries().find((candidate) => candidate.catalogId === catalogId);
  if (!entry) throw new Error(`no fixture entry ${catalogId}`);
  const release = entry.version ? releaseFor(entry, entry.version) : null;
  return {
    catalogId,
    description: 'A community port of the original console release.',
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
    updateAvailable: catalogId === 'qcat_emberfall' ? releaseFor(entry, '1.0.5') : null,
    noAssetReason: entry.noAssetReason,
  };
}

function renderPorts(ports: PortsViewPayload, locale = 'en') {
  const snapshot: Snapshot = createDefaultSnapshot();
  snapshot.ports = ports;
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  const view = render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider initialLocale={locale}>
        <PortsView />
      </I18nProvider>
    </QueryClientProvider>,
  );
  return { ...view, queryClient };
}

describe('Ports catalog', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
    vi.mocked(openPortSetup).mockClear();
    vi.spyOn(commands, 'portsDetail').mockImplementation(async (catalogId) =>
      detailForEntry(catalogId),
    );
    vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    vi.spyOn(commands, 'portsSetPreferences').mockResolvedValue();
    vi.spyOn(commands, 'portsRefresh').mockResolvedValue();
    vi.spyOn(commands, 'portsMarkSeen').mockResolvedValue();
    vi.spyOn(commands, 'portsCancel').mockResolvedValue();
    vi.spyOn(commands, 'openToolSource').mockResolvedValue();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('filters by search over names and game titles, and by the persisted toggles', async () => {
    renderPorts(makePorts());

    expect(screen.getByTestId('port-tile-qcat_aster')).toBeInTheDocument();
    expect(screen.getByTestId('port-tile-qcat_beyond')).toBeInTheDocument();

    fireEvent.change(screen.getByTestId('ports-search-input'), {
      target: { value: 'ashes' },
    });
    await waitFor(() =>
      expect(screen.queryByTestId('port-tile-qcat_beyond')).not.toBeInTheDocument(),
    );
    expect(screen.getByTestId('port-tile-qcat_aster')).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('ports-clear-search-btn'));
    await waitFor(() =>
      expect(screen.getByTestId('port-tile-qcat_beyond')).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByTestId('ports-guided-filter'));
    expect(commands.portsSetPreferences).toHaveBeenLastCalledWith({
      filters: { consoles: [], guided: true, verified: false, installed: false },
    });

    fireEvent.change(screen.getByTestId('ports-console-filter'), {
      target: { value: 'PlayStation 4' },
    });
    expect(commands.portsSetPreferences).toHaveBeenLastCalledWith({
      filters: { consoles: ['PlayStation 4'], guided: false, verified: false, installed: false },
    });
  });

  it('renders the remembered list view and installed filter, and persists a view change', () => {
    renderPorts(
      makePorts({
        preferences: {
          view: 'list',
          filters: { consoles: [], guided: false, verified: false, installed: true },
          lastParentFolder: null,
        },
      }),
    );

    expect(screen.getByTestId('ports-list-container')).toBeInTheDocument();
    expect(screen.getByTestId('port-row-qcat_emberfall')).toBeInTheDocument();
    expect(screen.queryByTestId('port-row-qcat_aster')).not.toBeInTheDocument();
    expect(screen.getByTestId('ports-filtered-count')).toHaveTextContent('1');

    fireEvent.click(screen.getByTestId('ports-view-grid-btn'));
    expect(commands.portsSetPreferences).toHaveBeenCalledWith({ view: 'grid' });
  });

  it('shows the unavailable state with an error-specific message and Retry', () => {
    renderPorts(
      makePorts({
        catalog: {
          state: 'unavailable',
          fetchedAt: null,
          entryCount: 0,
          error: 'unreachable',
        },
        entries: [],
        provenance: {},
      }),
    );

    expect(screen.getByTestId('ports-unavailable')).toBeInTheDocument();
    expect(
      screen.getByText(
        'The catalog server cannot be reached. Check your connection and try again.',
      ),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('ports-unavailable-retry'));
    expect(commands.portsRefresh).toHaveBeenCalledTimes(1);
  });

  it('shows the cached-catalog age when the snapshot is stale', () => {
    renderPorts(
      makePorts({
        catalog: {
          state: 'stale',
          fetchedAt: Math.floor(Date.now() / 1000) - 2 * 24 * 3600,
          entryCount: 6,
          error: 'unreachable',
        },
      }),
    );

    const banner = screen.getByTestId('banner-ports-stale');
    expect(banner).toHaveTextContent('Showing the cached catalog');
    expect(banner.textContent).toMatch(/day/);
  });

  it('keeps the upstream credit as text with no Quiver image', () => {
    renderPorts(makePorts());

    expect(screen.getByTestId('ports-credit')).toHaveTextContent(
      'Port catalog by quiverlauncher.com',
    );
    const images = Array.from(document.querySelectorAll('img'));
    expect(images.length).toBeGreaterThan(0);
    for (const image of images) {
      expect(image.getAttribute('src') ?? '').not.toMatch(/quiver/i);
      expect(image.getAttribute('alt') ?? '').not.toMatch(/quiver/i);
    }
    expect(document.querySelectorAll('svg[aria-label*="Quiver"]')).toHaveLength(0);
  });

  it('opens the guided setup for an installable entry', () => {
    renderPorts(makePorts());

    fireEvent.click(screen.getByTestId('port-tile-qcat_aster'));
    fireEvent.click(screen.getByTestId('port-install-btn'));

    expect(openPortSetup).toHaveBeenCalledWith('qcat_aster');
  });

  it('renders installed, withdrawn and no-asset entries with the right action and reason', async () => {
    renderPorts(makePorts());

    fireEvent.click(screen.getByTestId('port-tile-qcat_emberfall'));
    expect(await screen.findByTestId('port-open-library-btn')).toBeInTheDocument();
    expect(screen.getByTestId('port-installed-info')).toHaveTextContent('v1.0.4');
    expect(await screen.findByTestId('port-update-notice')).toHaveTextContent('v1.0.5');
    fireEvent.click(screen.getByTestId('port-open-library-btn'));
    expect(uiStore.state.activeDestination).toBe('library');
    expect(commands.libraryInput).toHaveBeenCalledWith({
      select: { key: 'drmfree:emberfall', mode: 'replace' },
    });

    fireEvent.click(screen.getByTestId('port-detail-close-btn'));
    fireEvent.click(screen.getByTestId('port-tile-qcat_distant'));
    expect(screen.getByTestId('port-install-btn')).toBeDisabled();
    expect(screen.getByTestId('port-install-reason')).toHaveTextContent(
      'This port was withdrawn upstream.',
    );
    expect(screen.getAllByTestId('port-badge-withdrawn').length).toBeGreaterThan(0);

    fireEvent.click(screen.getByTestId('port-detail-close-btn'));
    fireEvent.click(screen.getByTestId('port-tile-qcat_glass'));
    expect(screen.getByTestId('port-install-btn')).toBeDisabled();
    expect(screen.getByTestId('port-install-reason')).toHaveTextContent(
      'The Windows download is an installer, which GameNode does not run.',
    );
  });

  it('lists guided requirements with accepted versions, optional slots and the package option', async () => {
    renderPorts(makePorts());

    fireEvent.click(screen.getByTestId('port-tile-qcat_aster'));

    expect(await screen.findByTestId('port-requirements')).toHaveTextContent(
      'Aster Vale (CUSA10101) game folder',
    );
    expect(screen.getByTestId('port-requirement-accepted-game-folder')).toHaveTextContent(
      'CUSA10101 (01.04)',
    );
    expect(screen.getByTestId('port-requirement-required-settings-backup')).toHaveTextContent(
      'Optional',
    );
    expect(screen.getByTestId('port-package-option-game-folder')).toHaveTextContent(
      'Mock PKG Extractor',
    );
    expect(screen.getByTestId('port-package-option-game-folder')).toHaveTextContent('01.04');
    expect(screen.getByTestId('port-detail-links')).toHaveTextContent(
      'Aster Vale Revival',
    );
    expect(screen.getByTestId('port-detail-pane')).toHaveTextContent(
      'GPL-3.0-or-later',
    );
    fireEvent.click(screen.getByTestId('port-project-link'));
    expect(commands.openToolSource).toHaveBeenCalledWith(
      'https://example.invalid/aster',
    );
  });

  it('shows the install progress row with its phase, bar and Cancel', async () => {
    const runningJob: PortJobView = {
      jobId: 'job-running',
      catalogId: 'qcat_aster',
      slug: 'aster-vale-revival',
      name: 'Aster Vale: Revival',
      phase: 'downloading',
      outcome: null,
      interruptible: true,
      retryable: false,
      seen: false,
      startedAt: NOW,
    };
    const { queryClient } = renderPorts(makePorts({ jobs: [runningJob] }));
    queryClient.setQueryData(portProgressQueryKey('job-running'), {
      jobId: 'job-running',
      phase: 'downloading',
      bytesDone: 50,
      bytesTotal: 100,
    });

    fireEvent.click(screen.getByTestId('port-tile-qcat_aster'));

    expect(await screen.findByTestId('ports-progress-row')).toHaveAttribute(
      'data-phase',
      'downloading',
    );
    expect(screen.getByTestId('ports-progress-phase')).toHaveTextContent('Downloading');
    expect(screen.getByTestId('ports-progress-bar')).toHaveAttribute('aria-valuenow', '50');
    fireEvent.click(screen.getByTestId('ports-progress-cancel'));
    expect(commands.portsCancel).toHaveBeenCalledWith('job-running');
  });

  it('shows failure details and Retry for a failed job', async () => {
    const failedJob: PortJobView = {
      jobId: 'job-failed',
      catalogId: 'qcat_beyond',
      slug: 'beyond-the-pines',
      name: 'Beyond the Pines',
      phase: 'failed',
      outcome: 'failed',
      failure: { message: 'The archive could not be verified.', toolOutput: null },
      interruptible: false,
      retryable: true,
      seen: false,
      startedAt: NOW - 90,
      finishedAt: NOW - 60,
    };
    renderPorts(makePorts({ jobs: [failedJob] }));

    fireEvent.click(screen.getByTestId('port-tile-qcat_beyond'));

    expect(await screen.findByTestId('port-failure-details')).toHaveTextContent(
      'The archive could not be verified.',
    );
    expect(screen.getByTestId('ports-progress-phase')).toHaveTextContent('Install failed');
    fireEvent.click(screen.getByTestId('ports-progress-retry'));
    expect(openPortSetup).toHaveBeenCalledWith('qcat_beyond', {
      retryJobId: 'job-failed',
    });
  });

  it('renders the catalog in Brazilian Portuguese', () => {
    renderPorts(makePorts(), 'pt-BR');

    expect(screen.getByTestId('ports-search-input')).toHaveAttribute(
      'placeholder',
      'Pesquisar no catálogo de ports',
    );
    expect(screen.getByTestId('ports-credit')).toHaveTextContent(
      'Catálogo de ports do quiverlauncher.com',
    );
  });
});
