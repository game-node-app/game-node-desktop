import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type PluginInfo, type RememberedConfirmation } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginInventory } from './PluginInventory';
import { RememberedConfirmations } from './RememberedConfirmations';

const FINGERPRINT = 'cd'.repeat(32);

const launch: RememberedConfirmation = {
  id: 'r1',
  gameName: 'Space Game',
  operation: 'launch',
  approvedAt: 1_700_000_000,
};
const install: RememberedConfirmation = {
  id: 'r2',
  gameName: '<b>Other</b>‮ Game',
  operation: 'install',
  approvedAt: 1_700_100_000,
};

function wrap(node: React.ReactNode, locale = 'en') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>{node}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('RememberedConfirmations', () => {
  beforeEach(() => vi.restoreAllMocks());

  it('lists what the plugin may start without asking, as plain text', async () => {
    const list = vi
      .spyOn(commands, 'pluginRememberedConfirmations')
      .mockResolvedValue([launch, install]);
    wrap(<RememberedConfirmations pluginId="org.example" publisherFingerprint={FINGERPRINT} />);
    expect(await screen.findByText(/Start: Space Game/)).toBeInTheDocument();
    expect(list).toHaveBeenCalledWith('org.example', FINGERPRINT);
    const item = screen.getByText(/Open install flow:/);
    expect(item).toHaveTextContent('<b>Other</b> Game');
    expect(item.querySelector('b')).toBeNull();
  });

  it('says so when there is nothing, and when it cannot load', async () => {
    vi.spyOn(commands, 'pluginRememberedConfirmations').mockResolvedValue([]);
    const { unmount } = wrap(
      <RememberedConfirmations pluginId="org.example" publisherFingerprint={FINGERPRINT} />,
    );
    expect(await screen.findByText('This plugin has no remembered confirmations.')).toBeInTheDocument();
    unmount();
    vi.spyOn(commands, 'pluginRememberedConfirmations').mockRejectedValue(new Error('x'));
    wrap(<RememberedConfirmations pluginId="org.example" publisherFingerprint={FINGERPRINT} />);
    expect(await screen.findByRole('alert')).toHaveTextContent('could not be loaded');
  });

  it('revokes one confirmation and reads the list again from the host', async () => {
    const list = vi
      .spyOn(commands, 'pluginRememberedConfirmations')
      .mockResolvedValueOnce([launch, install])
      .mockResolvedValue([install]);
    const revoke = vi.spyOn(commands, 'pluginRevokeConfirmation').mockResolvedValue(true);
    wrap(<RememberedConfirmations pluginId="org.example" publisherFingerprint={FINGERPRINT} />);
    fireEvent.click(await screen.findByRole('button', { name: /Revoke permission to act on Space Game/ }));
    await waitFor(() => expect(revoke).toHaveBeenCalledWith('org.example', FINGERPRINT, 'r1'));
    await waitFor(() => expect(screen.queryByText(/Start: Space Game/)).not.toBeInTheDocument());
    expect(list).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/Open install flow:/)).toBeInTheDocument();
  });

  it('reports a failed revocation and keeps what the host says', async () => {
    vi.spyOn(commands, 'pluginRememberedConfirmations').mockResolvedValue([launch]);
    vi.spyOn(commands, 'pluginRevokeConfirmation').mockRejectedValue(new Error('x'));
    wrap(<RememberedConfirmations pluginId="org.example" publisherFingerprint={FINGERPRINT} />);
    fireEvent.click(await screen.findByRole('button', { name: /Revoke permission/ }));
    expect(await screen.findByText('The confirmation could not be revoked.')).toBeInTheDocument();
    expect(screen.getByText(/Start: Space Game/)).toBeInTheDocument();
  });

  it('is localized in Portuguese', async () => {
    vi.spyOn(commands, 'pluginRememberedConfirmations').mockResolvedValue([launch]);
    wrap(
      <RememberedConfirmations pluginId="org.example" publisherFingerprint={FINGERPRINT} />,
      'pt-BR',
    );
    expect(await screen.findByText(/Iniciar: Space Game/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Revogar a permissão para agir em Space Game/ })).toBeInTheDocument();
  });

  it('appears in the plugin details only for a plugin with a pinned publisher', async () => {
    const community: PluginInfo = {
      providerId: 'community',
      pluginId: 'org.example.community',
      pluginKind: 'provider',
      name: 'Community',
      source: 'community',
      status: 'installed',
      capabilities: ['game.launch'],
      session: 'active',
      nativeAccess: [],
      publisherFingerprint: FINGERPRINT,
    };
    const bundled: PluginInfo = { ...community, providerId: 'steam', pluginId: 'steam.plugin', name: 'Steam', source: 'official', publisherFingerprint: undefined };
    const list = vi.spyOn(commands, 'pluginRememberedConfirmations').mockResolvedValue([]);
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([bundled, community]);
    wrap(<PluginInventory />);
    // The bundled plugin is selected first: no list, and nothing was asked of the host.
    await screen.findAllByText('Steam');
    expect(screen.queryByText('Allowed without asking')).not.toBeInTheDocument();
    expect(list).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: /Community/ }));
    expect(await screen.findByText('Allowed without asking')).toBeInTheDocument();
    expect(list).toHaveBeenCalledWith('org.example.community', FINGERPRINT);
  });
});
