import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type PluginRuntimeApproval } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginRuntimeApprovals } from './PluginRuntimeApprovals';

const publisher = 'ab'.repeat(32);
const approvals: PluginRuntimeApproval[] = [
  {
    runtimeId: 'runtime-1',
    path: 'C:\\Emulators\\Dolphin\\Dolphin.exe',
    approvedAt: 1_760_000_000,
  },
  {
    runtimeId: 'runtime-2',
    path: '/usr/bin/dolphin-emu',
    approvedAt: 1_760_100_000,
  },
];

function renderPanel(locale = 'en') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginRuntimeApprovals pluginId="community.example" publisherFingerprint={publisher} />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('PluginRuntimeApprovals', () => {
  beforeEach(() => {
    vi.spyOn(commands, 'pluginRuntimeApprovals').mockResolvedValue(approvals);
    vi.spyOn(commands, 'pluginRuntimeRevoke').mockResolvedValue(true);
  });
  afterEach(() => vi.restoreAllMocks());

  it.each([
    ['en', 'Approved runtime programs', /Remove approval for/, /Approved .*2025/],
    ['pt-BR', 'Programas de runtime aprovados', /Remover aprovação de/, /Aprovado em .*2025/],
  ])('lists approved programs with a Remove action in %s', async (locale, heading, remove, date) => {
    renderPanel(locale);

    expect(await screen.findByRole('region', { name: heading })).toBeInTheDocument();
    expect(await screen.findByText('C:\\Emulators\\Dolphin\\Dolphin.exe')).toBeInTheDocument();
    expect(screen.getByText('/usr/bin/dolphin-emu')).toBeInTheDocument();
    expect(screen.getAllByText(date).length).toBeGreaterThan(0);
    expect(screen.getByText(/ask for a new program|pedir um novo programa/)).toBeInTheDocument();

    fireEvent.click(screen.getAllByRole('button', { name: remove })[0]);
    await waitFor(() => expect(commands.pluginRuntimeRevoke).toHaveBeenCalledWith(
      'community.example', publisher, 'runtime-1',
    ));
  });

  it('shows the empty state without a Remove action', async () => {
    vi.spyOn(commands, 'pluginRuntimeApprovals').mockResolvedValue([]);
    renderPanel('en');

    expect(await screen.findByText('This plugin has no approved runtime programs.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Remove approval for/ })).not.toBeInTheDocument();
  });

  it('reports a failed revocation without hiding the saved approval', async () => {
    vi.spyOn(commands, 'pluginRuntimeRevoke').mockRejectedValue(new Error('storage failed'));
    renderPanel('en');

    fireEvent.click((await screen.findAllByRole('button', { name: /Remove approval for/ }))[0]);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not update this program approval.',
    );
    expect(screen.getByText('/usr/bin/dolphin-emu')).toBeInTheDocument();
  });

  it('reports a failed list request', async () => {
    vi.spyOn(commands, 'pluginRuntimeApprovals').mockRejectedValue(new Error('storage failed'));
    renderPanel('en');

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not update this program approval.',
    );
  });
});
