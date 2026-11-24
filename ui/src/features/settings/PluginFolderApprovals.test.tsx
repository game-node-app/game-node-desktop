import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type PluginFolderApproval } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginFolderApprovals } from './PluginFolderApprovals';

const approvals: PluginFolderApproval[] = [
  {
    id: 'file-1', pluginId: 'community.example', publisherFingerprint: 'ab'.repeat(32),
    installationName: 'Space Game', kind: 'save', root: 'C:\\Games\\Space\\Saves',
    operations: ['read'], affectedGames: ['Space Game'], revoked: false, declaredByPlugin: true,
    logicalLibrary: false, libraryId: null, watching: null,
  },
  {
    id: 'root-1', pluginId: 'community.example', publisherFingerprint: 'ab'.repeat(32),
    installationName: 'Game roots', kind: 'library', root: 'C:\\Games',
    operations: ['read', 'plugin_discovery', 'core_executable_discovery'], affectedGames: [],
    revoked: false, declaredByPlugin: false, logicalLibrary: true, libraryId: 'library-1', watching: true,
  },
];

function renderPanel(locale = 'en') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginFolderApprovals pluginId="community.example" publisherFingerprint={'ab'.repeat(32)} />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('PluginFolderApprovals', () => {
  beforeEach(() => {
    vi.spyOn(commands, 'pluginFolderApprovals').mockResolvedValue(approvals);
    vi.spyOn(commands, 'pluginFolderChange').mockResolvedValue(undefined);
    vi.spyOn(commands, 'pluginFolderRevoke').mockResolvedValue(true);
    vi.spyOn(commands, 'pluginLibraryRootSetWatching').mockResolvedValue(true);
  });
  afterEach(() => vi.restoreAllMocks());

  it.each([
    ['en', 'Folder access and library roots', 'Stop Core monitoring'],
    ['pt-BR', 'Acesso a pastas e raízes de biblioteca', 'Parar monitoramento pelo Core'],
  ])('lists scopes and exposes separate file and Core monitoring controls in %s', async (locale, heading, stopWatching) => {
    renderPanel(locale);
    expect(await screen.findByRole('region', { name: heading })).toBeInTheDocument();
    expect(await screen.findByText('C:\\Games\\Space\\Saves')).toBeInTheDocument();
    expect(screen.getByText(/does not grant file bytes|não concede acesso aos arquivos/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: stopWatching }));
    await waitFor(() => expect(commands.pluginLibraryRootSetWatching).toHaveBeenCalledWith(
      'community.example', 'ab'.repeat(32), 'root-1', false,
    ));

    fireEvent.click(screen.getByRole('button', { name: locale === 'en' ? 'Change folder…' : 'Alterar pasta…' }));
    await waitFor(() => expect(commands.pluginFolderChange).toHaveBeenCalledWith(
      'community.example', 'ab'.repeat(32), 'file-1',
    ));
  });
});
