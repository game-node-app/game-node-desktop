import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import React, { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  commands,
  type PluginStorageAccounting,
  type StorageCleanupReport,
  type StorageCleanupReview,
} from '../../api';
import { appTheme } from '../../AppProviders';
import { I18nProvider } from '../../i18n';
import { PluginStorageAdmin } from './PluginStorageAdmin';

const poolPath = '/home/test/.local/share/gamenode/plugin-shared-data.gnpersistence';
const privatePath = '/home/test/.local/share/gamenode/plugin-data/cdcd/dev.example.producer/a/storage.sqlite3';

const accounting: PluginStorageAccounting = {
  profileId: 'a',
  locations: [
    {
      pluginId: 'dev.example.producer',
      kind: 'durable_shared',
      path: poolPath,
      scope: 'pool',
      fileBytes: 262_144,
      logicalBytes: 153_600,
      records: 12,
      protectedRecords: 0,
      archivedRecords: 0,
      oldestUsedAt: 1_700_000_000,
      maxBytes: 64 * 1024 * 1024,
      maxAgeSeconds: 30 * 24 * 60 * 60,
    },
    {
      pluginId: 'dev.example.producer',
      kind: 'private_persistence',
      path: privatePath,
      scope: 'namespace',
      fileBytes: 57_344,
      logicalBytes: 40_960,
      records: 5,
      protectedRecords: 2,
      archivedRecords: 1,
      oldestUsedAt: 1_700_000_000,
      maxBytes: 16 * 1024 * 1024,
      maxAgeSeconds: null,
    },
  ],
  pool: {
    path: poolPath,
    fileBytes: 262_144,
    logicalBytes: 153_600,
    overheadBytes: 108_544,
    owners: 1,
    maxOwnerBytes: 64 * 1024 * 1024,
  },
  privateRootBytes: 57_344,
};

const review: StorageCleanupReview = {
  token: 'review-token',
  generatedAt: 1_700_000_000,
  candidates: [
    {
      candidateId: 'candidate-durable',
      kind: 'durable_shared',
      pluginId: 'dev.example.producer',
      profileId: 'a',
      records: 12,
      protectedRecords: 0,
      archivedRecords: 0,
      logicalBytes: 153_600,
      physicalBytesEstimate: 262_144,
      oldestUpdatedAt: 1_700_000_000,
      maxBytes: 64 * 1024 * 1024,
      maxAgeSeconds: 30 * 24 * 60 * 60,
    },
    {
      candidateId: 'candidate-private',
      kind: 'private_persistence',
      pluginId: 'dev.example.producer',
      profileId: 'a',
      records: 5,
      protectedRecords: 2,
      archivedRecords: 1,
      logicalBytes: 40_960,
      physicalBytesEstimate: 57_344,
      oldestUpdatedAt: 1_700_000_000,
      maxBytes: 16 * 1024 * 1024,
      maxAgeSeconds: null,
    },
  ],
};

const report: StorageCleanupReport = {
  token: 'review-token',
  removedRecords: 12,
  reclaimedBytes: 153_600,
  skippedProtectedRecords: 0,
  results: [
    {
      candidateId: 'candidate-durable',
      kind: 'durable_shared',
      pluginId: 'dev.example.producer',
      status: 'completed',
      removedRecords: 12,
      reclaimedBytes: 153_600,
      error: null,
    },
  ],
};

function renderAdmin(locale = 'en', profileId: string | null = 'a') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginStorageAdmin profileId={profileId} />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
  return client;
}

function renderWithSibling() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const Harness: React.FC = () => {
    const [opened, setOpened] = useState(0);
    return (
      <>
        <PluginStorageAdmin profileId="a" />
        <button type="button" onClick={() => setOpened((value) => value + 1)}>
          Open another plugin
        </button>
        <p>opened {opened}</p>
      </>
    );
  };
  render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale="en">
          <Harness />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
  return client;
}

describe('PluginStorageAdmin', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('shows host-managed physical paths and overhead only in the host screen', async () => {
    vi.spyOn(commands, 'pluginStorageLocations').mockResolvedValue(accounting);
    renderAdmin();

    expect(await screen.findAllByText(poolPath)).toHaveLength(2);
    expect(screen.getByText(privatePath)).toBeInTheDocument();
    expect(screen.getByText('Physical 56 KB · logical 40 KB')).toBeInTheDocument();
    expect(screen.getByText('File overhead: 16 KB (SQLite metadata and free pages)'))
      .toBeInTheDocument();
    expect(
      screen.getByText('This file is shared by every plugin that keeps durable shared data; the pool overhead below is pool-wide.'),
    ).toBeInTheDocument();
    expect(
      screen.getByText('Physical 256 KB · logical 150 KB · overhead 106 KB · plugins 1'),
    ).toBeInTheDocument();
    expect(screen.getByText('Host administration')).toBeInTheDocument();
    expect(
      screen.getByText(/Plugin IPC receives logical sizes only and cannot read a managed path\./),
    ).toBeInTheDocument();
    expect(screen.getByText('Records: 5 · user-protected: 2')).toBeInTheDocument();
    // The per-plugin logical view stays on the plugin IPC-shaped surface; this screen is the
    // only one that reads the managed paths.
    expect(commands.pluginStorageLocations).toHaveBeenCalledTimes(1);
  });

  it('applies only the confirmed selection and reports the measured reclaimed bytes', async () => {
    vi.spyOn(commands, 'pluginStorageLocations').mockResolvedValue(accounting);
    vi.spyOn(commands, 'pluginStorageCleanupReview').mockResolvedValue(review);
    const apply = vi.spyOn(commands, 'pluginStorageCleanupApply').mockResolvedValue(report);
    renderAdmin();

    fireEvent.click(await screen.findByRole('button', { name: 'Review cleanup candidates' }));
    expect(await screen.findByText('2 candidates selected')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('checkbox', { name: /Private plugin data/ }));
    expect(screen.getByText('1 candidates selected')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Apply cleanup' }));
    expect(apply).not.toHaveBeenCalled();
    const dialog = await screen.findByRole('dialog');
    expect(within(dialog).getByText(/Remove the reviewed datasets of the 1 selected candidates/))
      .toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole('button', { name: 'Apply cleanup' }));

    await waitFor(() =>
      expect(apply).toHaveBeenCalledWith('review-token', ['candidate-durable'], true));
    expect(
      await screen.findByText(
        'Removed 12 records · reclaimed 150 KB · kept 0 protected records',
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('Completed: removed 12 records, reclaimed 150 KB'))
      .toBeInTheDocument();
    // The review is consumed; a new one is needed for anything else.
    expect(screen.queryByText('2 candidates selected')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Review cleanup candidates' }))
      .toBeInTheDocument();
  });

  it('renders each candidate of a partial failure instead of collapsing the report', async () => {
    vi.spyOn(commands, 'pluginStorageLocations').mockResolvedValue(accounting);
    vi.spyOn(commands, 'pluginStorageCleanupReview').mockResolvedValue(review);
    vi.spyOn(commands, 'pluginStorageCleanupApply').mockResolvedValue({
      ...report,
      removedRecords: 12,
      reclaimedBytes: 153_600,
      skippedProtectedRecords: 2,
      results: [
        report.results[0],
        {
          candidateId: 'candidate-private',
          kind: 'private_persistence',
          pluginId: 'dev.example.producer',
          status: 'failed',
          removedRecords: 0,
          reclaimedBytes: 0,
          error: 'plugin identity changed since the cleanup review',
        },
      ],
    });
    renderAdmin();

    fireEvent.click(await screen.findByRole('button', { name: 'Review cleanup candidates' }));
    await screen.findByText('2 candidates selected');
    fireEvent.click(screen.getByRole('button', { name: 'Apply cleanup' }));
    fireEvent.click(within(await screen.findByRole('dialog'))
      .getByRole('button', { name: 'Apply cleanup' }));

    expect(
      await screen.findByText(
        'Removed 12 records · reclaimed 150 KB · kept 2 protected records',
      ),
    ).toBeInTheDocument();
    expect(screen.getByText('Completed: removed 12 records, reclaimed 150 KB'))
      .toBeInTheDocument();
    expect(screen.getByText('Failed: plugin identity changed since the cleanup review'))
      .toBeInTheDocument();
  });

  it('keeps the rest of the settings usable while a slow review runs', async () => {
    vi.spyOn(commands, 'pluginStorageLocations').mockResolvedValue(accounting);
    let finishReview: (value: StorageCleanupReview) => void = () => undefined;
    vi.spyOn(commands, 'pluginStorageCleanupReview').mockImplementation(
      () => new Promise<StorageCleanupReview>((resolve) => { finishReview = resolve; }),
    );
    renderWithSibling();

    fireEvent.click(await screen.findByRole('button', { name: 'Review cleanup candidates' }));
    expect(screen.getByText('Reading cleanup candidates…')).toBeInTheDocument();

    // The accounting query settled before the review was requested, and unrelated plugin
    // navigation is not blocked by the pending host command.
    expect((await screen.findAllByText(poolPath)).length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole('button', { name: 'Open another plugin' }));
    expect(await screen.findByText('opened 1')).toBeInTheDocument();
    expect(screen.getByText('Reading cleanup candidates…')).toBeInTheDocument();

    finishReview(review);
    expect(await screen.findByText('2 candidates selected')).toBeInTheDocument();
  });

  it('renders the Portuguese accounting and cleanup copy', async () => {
    vi.spyOn(commands, 'pluginStorageLocations').mockResolvedValue(accounting);
    renderAdmin('pt-BR');

    expect(await screen.findByText('Dados de plugins neste PC')).toBeInTheDocument();
    expect(screen.getByText('Administração do host')).toBeInTheDocument();
    expect(await screen.findByText('Físico 56 KB · lógico 40 KB')).toBeInTheDocument();
    expect(screen.getByText('Sobrecarga do arquivo: 16 KB (metadados e páginas livres do SQLite)'))
      .toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Revisar candidatos de limpeza' }))
      .toBeInTheDocument();
  });

  it('asks for sign-in instead of reading managed locations without a profile', async () => {
    const locations = vi.spyOn(commands, 'pluginStorageLocations').mockResolvedValue(accounting);
    renderAdmin('en', null);

    expect(await screen.findByText(/sign in/i)).toBeInTheDocument();
    expect(locations).not.toHaveBeenCalled();
    expect(screen.queryByText(poolPath)).not.toBeInTheDocument();
  });
});
