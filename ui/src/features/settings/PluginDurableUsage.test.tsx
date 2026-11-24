import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { commands, type PluginDurableUsage } from '../../api';
import { appTheme } from '../../AppProviders';
import { I18nProvider } from '../../i18n';
import { PluginDurableUsage as PluginDurableUsageSection } from './PluginDurableUsage';

const usage: PluginDurableUsage = {
  pluginId: 'org.example.producer',
  profileId: 'a',
  recordCount: 3,
  logicalBytes: 4096,
  maxBytes: 1024 * 1024,
  maxAgeSeconds: 30 * 24 * 60 * 60,
  oldestUpdatedAt: 1_700_000_000,
};

function renderUsage(locale = 'en') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginDurableUsageSection pluginId="org.example.producer" profileId="a" />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
  return client;
}

describe('PluginDurableUsage', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('shows logical usage and limits without disclosing a physical path', async () => {
    vi.spyOn(commands, 'pluginDurableUsage').mockResolvedValue(usage);
    renderUsage();

    expect(await screen.findByText('Records: 3')).toBeInTheDocument();
    expect(screen.getByText('Retained size: 4 KB')).toBeInTheDocument();
    expect(screen.getByText('Size limit: 1 MB')).toBeInTheDocument();
    expect(screen.getByText('Age limit: 30 days')).toBeInTheDocument();
    const text = screen.getByRole('region', { name: 'Retained shared data' }).textContent ?? '';
    for (const leaked of ['/home', '\\\\', '.sqlite3', 'plugin-data', 'plugin-shared-data']) {
      expect(text).not.toContain(leaked);
    }
  });

  it('requires confirmation before deleting and reports the reclaimed size', async () => {
    vi.spyOn(commands, 'pluginDurableUsage').mockResolvedValue(usage);
    const remove = vi.spyOn(commands, 'pluginDurableDelete').mockResolvedValue({
      pluginId: usage.pluginId,
      profileId: usage.profileId,
      removedRecords: 3,
      reclaimedBytes: 4096,
      usage: { ...usage, recordCount: 0, logicalBytes: 0 },
    });
    renderUsage();

    fireEvent.click(await screen.findByRole('button', { name: 'Delete retained data' }));
    expect(remove).not.toHaveBeenCalled();
    expect(screen.getByText(/only this plugin's durable shared records/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Delete data' }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith('org.example.producer', 'a', true));
    expect(await screen.findByText('Deleted 4 KB of retained shared data.')).toBeInTheDocument();
    expect(screen.getByText('This plugin retains no durable shared records.')).toBeInTheDocument();
  });

  it('renders the Portuguese limits and cleanup copy', async () => {
    vi.spyOn(commands, 'pluginDurableUsage').mockResolvedValue(usage);
    renderUsage('pt-BR');

    expect(await screen.findByText('Registros: 3')).toBeInTheDocument();
    expect(screen.getByText('Tamanho retido: 4 KB')).toBeInTheDocument();
    expect(screen.getByText('Limite de idade: 30 dias')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Excluir dados retidos' })).toBeInTheDocument();
  });
});
