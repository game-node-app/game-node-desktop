import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultSnapshot,
  type Candidate,
  type Snapshot,
  type WatchView,
} from '../../../api';
import { I18nProvider } from '../../../i18n';
import { resetUiStore } from '../../../store/uiStore';
import { DrmFreePanel } from './DrmFreePanel';

function watch(overrides: Partial<WatchView> = {}): WatchView {
  return {
    id: 'w-1',
    path: 'D:\\Jogos',
    enabled: true,
    entryCount: 18,
    lastCheckedAt: Math.floor(Date.now() / 1000) - 60,
    state: 'idle',
    unavailableSince: null,
    ...overrides,
  };
}

function candidate(ignored: boolean, index: number): Candidate {
  return {
    fingerprint: `fp-${index}`,
    watch: 'w-1',
    root: `D:\\Jogos\\Game ${index}`,
    title: `Game ${index}`,
    targets: [],
    evidence: [],
    ignored,
    incomplete: false,
  };
}

function renderPanel(
  localGames: Partial<Snapshot['localGames']> = {},
  platform: 'windows' | 'linux' = 'windows',
  locale = 'en',
) {
  resetUiStore();
  const snapshot: Snapshot = {
    ...createDefaultSnapshot(),
    localGames: {
      ...createDefaultSnapshot().localGames,
      ...localGames,
    },
  };
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  queryClient.setQueryData(APP_INFO_QUERY_KEY, {
    locale,
    systemLocale: locale,
    version: '0.2.0',
    buildDate: '2026-01-01',
    osVersion: 'Windows 11',
    arch: 'x86_64',
    backgroundSupported: platform === 'windows',
    platform,
  });
  return {
    queryClient,
    ...render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={locale}>
            <DrmFreePanel />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    ),
  };
}

describe('DrmFreePanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('lists watched folders with their game count, last check and state', () => {
    renderPanel({
      watches: [
        watch(),
        watch({
          id: 'w-2',
          path: 'E:\\Itch',
          entryCount: 9,
          lastCheckedAt: Math.floor(Date.now() / 1000) - 3_600,
        }),
      ],
    });

    expect(screen.getByText('D:\\Jogos')).toBeInTheDocument();
    expect(screen.getByText('E:\\Itch')).toBeInTheDocument();
    expect(screen.getByText(/18 games · last checked/)).toBeInTheDocument();
    expect(screen.getAllByText('Watching')).toHaveLength(2);
  });

  it('keeps an unavailable folder and hides its Check action', () => {
    renderPanel({
      watches: [
        watch(),
        watch({
          id: 'w-2',
          path: 'F:\\Portable',
          entryCount: 5,
          state: 'unavailable',
          unavailableSince: Math.floor(Date.now() / 1000) - 86_400,
        }),
      ],
    });

    const unavailable = screen.getByTestId('settings-watch-w-2');
    expect(within(unavailable).getByText(/5 games · drive disconnected since/)).toBeInTheDocument();
    expect(within(unavailable).getByText('Unavailable')).toBeInTheDocument();
    expect(within(unavailable).queryByRole('button', { name: 'Check' })).toBeNull();
    expect(screen.getAllByRole('button', { name: 'Check' })).toHaveLength(1);
  });

  it('checks only the selected folder', () => {
    const check = vi.spyOn(commands, 'checkLocalWatch').mockResolvedValue(undefined);
    renderPanel({
      watches: [watch(), watch({ id: 'w-2', path: 'E:\\Itch' })],
    });

    const row = screen.getByTestId('settings-watch-w-2');
    fireEvent.click(within(row).getByRole('button', { name: 'Check' }));

    expect(check).toHaveBeenCalledWith('w-2');
  });

  it('checks every folder and adds one through the native picker', () => {
    const checkAll = vi.spyOn(commands, 'rescanLocalFolders').mockResolvedValue(undefined);
    const addFolder = vi
      .spyOn(commands, 'pickLocalWatchFolder')
      .mockResolvedValue(undefined);
    renderPanel({ watches: [watch()] });

    fireEvent.click(screen.getByRole('button', { name: 'Add folder…' }));
    fireEvent.click(screen.getByRole('button', { name: 'Check all now' }));

    expect(addFolder).toHaveBeenCalledTimes(1);
    expect(checkAll).toHaveBeenCalledTimes(1);
  });

  it('stops watching a folder without removing its registered games', () => {
    const remove = vi.spyOn(commands, 'removeLocalWatch').mockResolvedValue(undefined);
    renderPanel({ watches: [watch()] });

    fireEvent.click(screen.getByRole('button', { name: 'Stop watching' }));

    expect(remove).toHaveBeenCalledWith('w-1');
  });

  it('applies the discovery preferences immediately', () => {
    const preference = vi
      .spyOn(commands, 'setAppPreference')
      .mockResolvedValue(undefined);
    renderPanel({ watches: [watch()] });

    fireEvent.click(
      screen.getByRole('switch', { name: 'Check when GameNode starts' }),
    );
    expect(preference).toHaveBeenCalledWith({ discoveryOnStart: false });

    fireEvent.click(
      screen.getByRole('switch', { name: 'Watch for changes in real time' }),
    );
    expect(preference).toHaveBeenCalledWith({ discoveryRealtime: false });
  });

  it('rolls a discovery preference back when saving fails', async () => {
    vi.spyOn(commands, 'setAppPreference').mockRejectedValue('offline');
    renderPanel({ watches: [watch()] });

    const toggle = screen.getByRole('switch', {
      name: 'Check when GameNode starts',
    });
    fireEvent.click(toggle);

    expect(
      await screen.findByText(
        'Could not save this preference. The control was restored to the saved value.',
      ),
    ).toBeInTheDocument();
    await waitFor(() => expect(toggle).toBeChecked());
  });

  it('shows the ignored discoveries count and resets it', () => {
    const reset = vi.spyOn(commands, 'resetLocalIgnores').mockResolvedValue(undefined);
    renderPanel({
      watches: [watch()],
      candidates: [0, 1, 2, 3].map((index) => candidate(true, index)),
    });

    expect(
      screen.getByText(/4 items marked/),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole('button', { name: 'Reset ignored discoveries' }),
    );
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it('adds a game from its executable', () => {
    const pick = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue(undefined);
    renderPanel({ watches: [] });

    fireEvent.click(screen.getByRole('button', { name: 'Choose executable…' }));

    expect(pick).toHaveBeenCalledTimes(1);
  });

  it('shows the empty state when no folder is watched', () => {
    renderPanel({ watches: [] });

    expect(screen.getByText('No watched folders')).toBeInTheDocument();
  });

  it('explains that the topic is unavailable on unsupported systems', () => {
    renderPanel({ watches: [watch()] }, 'linux');

    expect(screen.getByText('Unavailable on this system')).toBeInTheDocument();
    expect(
      screen.getByText('Local discovery is available in the Windows build.'),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add folder…' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Check all now' })).toBeNull();
  });

  it('localizes the folder state and actions in pt-BR', () => {
    renderPanel({ watches: [watch()] }, 'windows', 'pt-BR');

    expect(screen.getByText('Monitorando')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Verificar' })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Parar de monitorar' }),
    ).toBeInTheDocument();
  });
});
