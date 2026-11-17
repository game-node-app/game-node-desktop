import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultSnapshot,
  type BackupUsage,
  type Snapshot,
} from '../../../api';
import { I18nProvider } from '../../../i18n';
import { resetUiStore, uiStore } from '../../../store/uiStore';
import { BackupsPanel } from './BackupsPanel';

const USAGE: BackupUsage = {
  totalBytes: 3 * 1024 * 1024,
  totalVersions: 4,
  games: [
    {
      entryKey: 'manual:distant',
      title: 'Distant Skies: Recompiled',
      coverFile: 'distant.jpg',
      bytes: 2 * 1024 * 1024,
      versions: 2,
    },
    {
      entryKey: 'manual:garden',
      title: 'Glass Garden: Enhanced',
      coverFile: null,
      bytes: 1 * 1024 * 1024,
      versions: 2,
    },
  ],
};

function renderPanel(
  overrides: Partial<Snapshot> = {},
  platform: 'windows' | 'linux' = 'windows',
  locale = 'en',
) {
  resetUiStore();
  const defaults = createDefaultSnapshot().localGames;
  const snapshot: Snapshot = {
    ...createDefaultSnapshot(),
    ...overrides,
    localGames: {
      ...defaults,
      ...(overrides.localGames ?? {}),
      settings: {
        automatic: true,
        destination: '/saved/backups',
        retention: 10,
        ...(overrides.localGames?.settings ?? {}),
      },
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
    snapshot,
    ...render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={locale}>
            <BackupsPanel />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    ),
  };
}

describe('BackupsPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.spyOn(commands, 'localBackupUsage').mockResolvedValue(USAGE);
  });

  it('shows automatic protection, the destination and the copies kept', async () => {
    renderPanel();

    expect(screen.getByRole('heading', { name: 'Automatic protection' })).toBeInTheDocument();
    expect(
      screen.getByRole('switch', { name: 'Protect saves automatically' }),
    ).toBeChecked();
    expect(screen.getByLabelText('Backup destination')).toHaveValue('/saved/backups');
    expect(screen.getByTestId('settings-backups-copies')).toHaveTextContent('10 copies');
    expect(
      screen.getByText(/Backups stay on this PC/),
    ).toBeInTheDocument();
    expect(await screen.findByText('Distant Skies: Recompiled')).toBeInTheDocument();
  });

  it('keeps text and copies edits pending with changed dots and Before:', () => {
    renderPanel();

    const destination = screen.getByLabelText('Backup destination');
    fireEvent.change(destination, { target: { value: '/new/backups' } });

    expect(uiStore.state.settingsDraft).toEqual({
      topic: 'backups',
      values: { destination: '/new/backups' },
    });
    expect(screen.getByText('Before: /saved/backups')).toBeInTheDocument();
    expect(
      document.querySelectorAll('[data-changed="true"]').length,
    ).toBeGreaterThanOrEqual(1);
  });

  it('changes the default copies kept per game', async () => {
    renderPanel();

    fireEvent.click(screen.getByTestId('settings-backups-copies'));
    fireEvent.click(await screen.findByRole('option', { name: '20 copies' }));

    expect(uiStore.state.settingsDraft).toEqual({
      topic: 'backups',
      values: { retention: 20 },
    });
  });

  it('shows a destination error below the field and keeps the draft', () => {
    renderPanel();
    act(() => {
      uiStore.setState((previous) => ({
        ...previous,
        settingsDraft: {
          topic: 'backups',
          values: { destination: '/not/writable' },
        },
        settingsDraftError: 'invalid destination',
      }));
    });

    expect(screen.getByLabelText('Backup destination')).toHaveValue('/not/writable');
    expect(screen.getByRole('alert')).toHaveTextContent('invalid destination');
    expect(uiStore.state.settingsDraft).toEqual({
      topic: 'backups',
      values: { destination: '/not/writable' },
    });
  });

  it('shows total and per-game space with covers, largest first', async () => {
    renderPanel();

    expect(await screen.findByText('3 MB · 4 copies')).toBeInTheDocument();
    expect(screen.getByText('2 MB · 2 copies')).toBeInTheDocument();
    expect(screen.getByText('1 MB · 2 copies')).toBeInTheDocument();
    expect(screen.getByText('Distant Skies: Recompiled')).toBeInTheDocument();
    expect(screen.getByText('Glass Garden: Enhanced')).toBeInTheDocument();

    const cover = document.querySelector('img');
    expect(cover).not.toBeNull();
    expect(cover).toHaveAttribute('loading', 'lazy');
    expect(cover).toHaveAttribute('decoding', 'async');
    expect(cover?.getAttribute('src')).toContain('/cover/distant.jpg');
    expect(screen.queryByText(/of 5 GB/)).toBeNull();
    expect(screen.queryByText(/%/)).toBeNull();
  });

  it('refreshes the usage after a backup job changes', async () => {
    const usage = vi.mocked(commands.localBackupUsage);
    const { queryClient, snapshot } = renderPanel();
    await waitFor(() => expect(usage).toHaveBeenCalledTimes(1));

    act(() => {
      queryClient.setQueryData<Snapshot>(SNAPSHOT_QUERY_KEY, {
        ...snapshot,
        localGames: {
          ...snapshot.localGames,
          jobs: [
            {
              id: 'job-1',
              installation: 'distant',
              profile: 'profile-1',
              revision: 1,
              destination: '/saved/backups',
              state: 'successful',
              attempts: 1,
              readyAt: Math.floor(Date.now() / 1000),
              backupId: 'backup-1',
            },
          ],
        },
      });
    });

    await waitFor(() => expect(usage).toHaveBeenCalledTimes(2));
  });

  it('shows the empty state when no backup exists', async () => {
    vi.mocked(commands.localBackupUsage).mockResolvedValue({
      totalBytes: 0,
      totalVersions: 0,
      games: [],
    });
    renderPanel();

    expect(await screen.findByText('No backups yet')).toBeInTheDocument();
  });

  it('reports when the space used cannot be read', async () => {
    vi.mocked(commands.localBackupUsage).mockRejectedValue('io');
    renderPanel();

    expect(
      await screen.findByText('Could not read the backup space used.'),
    ).toBeInTheDocument();
  });

  it('explains that backups are unavailable on unsupported systems', () => {
    renderPanel({}, 'linux');

    expect(screen.getByText('Unavailable on this system')).toBeInTheDocument();
    expect(
      screen.getByText('Local backups are available in the Windows build.'),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText('Backup destination')).toBeNull();
    expect(commands.localBackupUsage).not.toHaveBeenCalled();
  });

  it('localizes the copies and space rows in pt-BR', async () => {
    renderPanel({}, 'windows', 'pt-BR');

    expect(screen.getByTestId('settings-backups-copies')).toHaveTextContent('10 cópias');
    expect(await screen.findByText('3 MB · 4 cópias')).toBeInTheDocument();
    expect(screen.getByText('Proteção automática')).toBeInTheDocument();
  });
});
