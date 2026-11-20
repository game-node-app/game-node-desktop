import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type AppInfo,
  type AppPreferences,
  type GameToolView,
  type LibraryView,
  type Row,
  type ToolCacheEntryView,
  type ToolCatalogItemView,
  type ToolsView,
} from '../../../api';
import { I18nProvider } from '../../../i18n';
import {
  openSettingsModal,
  resetUiStore,
  uiStore,
} from '../../../store/uiStore';
import { ToastContainer } from '../ToastContainer';
import { ToolsPanel } from './ToolsPanel';

const CATALOG: ToolCatalogItemView[] = [
  {
    id: 'optiscaler',
    name: 'OptiScaler',
    summary:
      'FSR, XeSS and DLSS upscaling in compatible games. Install it from the game’s panel.',
    license: 'GPL-3.0',
    sourceUrl: 'https://github.com/optiscaler/OptiScaler',
  },
];

const GAME_TOOL: GameToolView = {
  toolId: 'optiscaler',
  name: 'OptiScaler',
  channel: 'stable',
  version: '0.7.4',
  proxyDll: 'dxgi.dll',
  targetExe: 'game.exe',
  state: 'installed',
  summary: 'Installed version 0.7.4',
};

const CACHE_ENTRIES: ToolCacheEntryView[] = [
  {
    toolId: 'optiscaler',
    version: '0.7.4',
    channel: 'stable',
    bytes: 40 * 1024 * 1024,
    sha256Verified: true,
    inUse: true,
  },
  {
    toolId: 'optiscaler',
    version: '0.7.3',
    channel: 'stable',
    bytes: 30 * 1024 * 1024,
    sha256Verified: true,
    inUse: false,
  },
  {
    toolId: 'optiscaler',
    version: '0.8.0-pre1',
    channel: 'nightly',
    bytes: 20 * 1024 * 1024,
    sha256Verified: false,
    inUse: false,
  },
];

function gameRow(key: string, title: string): Row {
  return {
    playtime: { source: 'internal', seconds: 0, lastPlayedAt: null },
    running: false,
    key,
    title,
    detectedName: title,
    matchedName: null,
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    status: 'unmatched',
    coverFile: null,
    releaseYear: null,
    addedAt: 0,
    isFavorite: false,
    hidden: false,
    gamenode: null,
  };
}

function makeLibrary(): LibraryView {
  return {
    ...createDefaultLibraryView(),
    rows: [gameRow('steam:1', 'Hades'), gameRow('steam:2', 'Celeste')],
  };
}

function makeTools(overrides: Partial<ToolsView> = {}): ToolsView {
  return {
    installedByEntry: {
      'steam:1': [GAME_TOOL],
      'steam:2': [GAME_TOOL],
    },
    modal: null,
    totalCacheBytes: 90 * 1024 * 1024,
    totalCacheArchives: 3,
    achievementIconCacheBytes: 0,
    catalog: CATALOG,
    cacheEntries: CACHE_ENTRIES,
    ...overrides,
  };
}

const APP_INFO: AppInfo = {
  locale: 'en',
  systemLocale: 'en',
  version: '0.9.0',
  buildDate: '2026-11-10',
  osVersion: 'Windows 11 Pro (build 26100)',
  arch: 'x64',
  backgroundSupported: true,
  platform: 'linux',
  logDirectory: '/tmp/gamenode-logs',
};

interface RenderOptions {
  platform?: 'windows' | 'linux';
  preferences?: Partial<AppPreferences>;
  tools?: Partial<ToolsView>;
  locale?: string;
}

function renderTools(options: RenderOptions = {}) {
  resetUiStore();
  openSettingsModal('tools');
  const platform = options.platform ?? 'linux';
  const defaults = createDefaultSnapshot();
  const snapshot = {
    ...defaults,
    preferences: {
      ...defaults.preferences,
      ...options.preferences,
    },
    library: makeLibrary(),
    tools: makeTools(options.tools),
  };
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  queryClient.setQueryData(APP_INFO_QUERY_KEY, {
    ...APP_INFO,
    platform,
    backgroundSupported: platform === 'windows',
  });
  return {
    queryClient,
    snapshot,
    ...render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={options.locale ?? 'en'}>
            <ToastContainer />
            <ToolsPanel />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    ),
  };
}

describe('ToolsPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    resetUiStore();
  });

  it('shows the supported tool, its licence/source and the games using it', async () => {
    const openSource = vi
      .spyOn(commands, 'openToolSource')
      .mockResolvedValue(undefined);
    renderTools();

    expect(
      screen.getByRole('heading', { level: 3, name: 'Supported tools' }),
    ).toBeInTheDocument();
    expect(screen.getByText('OptiScaler')).toBeInTheDocument();
    expect(
      screen.getByText(/FSR, XeSS and DLSS upscaling/),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'License GPL-3.0 · Source code' }),
    ).toBeInTheDocument();
    expect(screen.getByText('In 2 games')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'View games' })).toBeInTheDocument();

    fireEvent.click(
      screen.getByRole('button', { name: 'License GPL-3.0 · Source code' }),
    );
    expect(openSource).toHaveBeenCalledWith(
      'https://github.com/optiscaler/OptiScaler',
    );
  });

  it('closes Settings and selects a game from the View games menu', async () => {
    const libraryInput = vi
      .spyOn(commands, 'libraryInput')
      .mockResolvedValue(undefined);
    renderTools();

    fireEvent.click(screen.getByRole('button', { name: 'View games' }));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Hades' }));

    expect(uiStore.state.settingsOpen).toBe(false);
    expect(uiStore.state.activeDestination).toBe('library');
    expect(libraryInput).toHaveBeenCalledWith({
      select: { key: 'steam:1', mode: 'replace' },
    });
  });

  it('lists every cached version with its in-use, channel and digest state', () => {
    renderTools();

    expect(screen.getByText('90 MB · 3 cached versions')).toBeInTheDocument();
    expect(screen.getByText('OptiScaler 0.7.4')).toBeInTheDocument();
    expect(screen.getByText('OptiScaler 0.7.3')).toBeInTheDocument();
    expect(screen.getByText('OptiScaler 0.8.0-pre1')).toBeInTheDocument();
    expect(screen.getByText('In use')).toBeInTheDocument();
    const cacheList = screen.getByRole('list', { name: 'Download cache' });
    expect(within(cacheList).getByText('Nightly')).toBeInTheDocument();
    expect(screen.getAllByText(/SHA-256 verified/)).toHaveLength(2);
    expect(screen.getByText('40 MB · SHA-256 verified')).toBeInTheDocument();
    expect(screen.getByText('20 MB')).toBeInTheDocument();
  });

  it('applies the automatic-check and default-channel preferences optimistically', () => {
    const setPreference = vi
      .spyOn(commands, 'setAppPreference')
      .mockResolvedValue(undefined);
    renderTools({
      preferences: { toolAutoCheck: true, toolDefaultChannel: 'stable' },
    });

    const autoCheck = screen.getByRole('switch', {
      name: 'Check for updates automatically',
    });
    expect(autoCheck).toBeChecked();
    fireEvent.click(autoCheck);
    expect(autoCheck).not.toBeChecked();
    expect(setPreference).toHaveBeenCalledWith({ toolAutoCheck: false });

    const nightly = screen.getByRole('radio', { name: 'Nightly' });
    fireEvent.click(nightly);
    expect(nightly).toBeChecked();
    expect(setPreference).toHaveBeenCalledWith({
      toolDefaultChannel: 'nightly',
    });
  });

  it('restores a failed automatic-check change and shows the panel error', async () => {
    vi.spyOn(commands, 'setAppPreference').mockRejectedValue('invalid');
    renderTools({ preferences: { toolAutoCheck: true } });

    const autoCheck = screen.getByRole('switch', {
      name: 'Check for updates automatically',
    });
    fireEvent.click(autoCheck);

    expect(
      await screen.findByTestId('settings-tools-error'),
    ).toHaveTextContent('The control was restored to the saved value');
    expect(autoCheck).toBeChecked();
  });

  it('clears the tool cache and reports a successful or failed clear', async () => {
    const clear = vi
      .spyOn(commands, 'clearToolCache')
      .mockResolvedValue(undefined);
    renderTools();

    fireEvent.click(screen.getByRole('button', { name: 'Clear cache' }));
    await waitFor(() => expect(clear).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Tool cache cleared')).toBeInTheDocument();

    clear.mockRejectedValueOnce('io');
    fireEvent.click(screen.getByRole('button', { name: 'Clear cache' }));
    expect(
      await screen.findByText('Could not clear the tool cache. Try again.'),
    ).toBeInTheDocument();
  });

  it('shows the Windows-only helper tools and ports cache', async () => {
    const portsCache = vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
      downloadsBytes: 734_003_200,
      helperTools: [
        {
          id: 'ps4-pkg',
          name: 'Fixture PKG extractor',
          license: 'GPL-3.0',
          sourceUrl: 'https://github.com/example/fixture-pkg',
          version: 'v2.4.1',
          cacheBytes: 18_874_368,
        },
      ],
      installActive: false,
    });
    renderTools({ platform: 'windows' });

    expect(await screen.findByText('Fixture PKG extractor')).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { level: 3, name: 'Helper tools' }),
    ).toBeInTheDocument();
    expect(screen.getByTestId('ports-helper-tools')).toHaveTextContent(
      'Fixture PKG extractor',
    );
    expect(screen.getByText('Pinned version v2.4.1')).toBeInTheDocument();
    expect(screen.getByText('18 MB')).toBeInTheDocument();
    expect(
      screen.getByRole('heading', { level: 3, name: 'Ports cache' }),
    ).toBeInTheDocument();
    expect(screen.getByTestId('ports-downloads-size')).toHaveTextContent(
      '700 MB',
    );
    expect(portsCache).toHaveBeenCalled();
  });

  it('clears the ports cache without touching installed ports', async () => {
    vi.spyOn(commands, 'portsToolsCache').mockResolvedValue({
      downloadsBytes: 734_003_200,
      helperTools: [],
      installActive: false,
    });
    const clearPorts = vi.spyOn(commands, 'portsClearCache').mockResolvedValue({
      downloadsBytes: 0,
      helperTools: [],
      installActive: false,
    });
    renderTools({ platform: 'windows' });

    expect(await screen.findByText('700 MB')).toBeInTheDocument();
    const clear = screen.getByTestId('ports-clear-cache');
    expect(clear).toBeEnabled();
    fireEvent.click(clear);

    await waitFor(() => expect(clearPorts).toHaveBeenCalledTimes(1));
    expect(await screen.findByText('Port cache cleared')).toBeInTheDocument();
    expect(screen.getByTestId('ports-downloads-size')).toHaveTextContent('0 B');
  });

  it('keeps the PC ports cards off non-Windows builds', () => {
    const portsCache = vi.spyOn(commands, 'portsToolsCache');
    renderTools({ platform: 'linux' });

    expect(screen.queryByText('Helper tools')).not.toBeInTheDocument();
    expect(screen.queryByText('Ports cache')).not.toBeInTheDocument();
    expect(screen.queryByTestId('ports-clear-cache')).not.toBeInTheDocument();
    expect(portsCache).not.toHaveBeenCalled();
  });

  it('localizes the tool card and cache in pt-BR', () => {
    renderTools({ locale: 'pt-BR' });

    expect(
      screen.getByRole('heading', { level: 3, name: 'Ferramentas suportadas' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Em 2 jogos')).toBeInTheDocument();
    expect(screen.getByText('Em uso')).toBeInTheDocument();
    expect(
      screen.getByText('90 MB · 3 versões em cache'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /Licença GPL-3.0 · Código-fonte/ }),
    ).toBeInTheDocument();
  });

  it('shows the unsupported-cache empty copy when no archives are cached', () => {
    renderTools({
      tools: {
        totalCacheBytes: 0,
        totalCacheArchives: 0,
        cacheEntries: [],
        installedByEntry: {},
      },
    });

    expect(screen.getByText('0 B · 0 cached versions')).toBeInTheDocument();
    expect(screen.getByText('Not installed')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Clear cache' }),
    ).toBeDisabled();
  });
});
