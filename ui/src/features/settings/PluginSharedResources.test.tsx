import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands, type PluginSharedResourceReview } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginSharedResources } from './PluginSharedResources';

const publisher = 'ab'.repeat(32);

const reviews: PluginSharedResourceReview[] = [
  {
    pluginId: 'community.example', publisherFingerprint: publisher, profileId: 'profile-a',
    profileName: 'Dolphin card', gameId: '3', installationId: 'id-fixture:3', runtimeId: 'org.dolphin',
    resourceId: 'memory-card', displayName: 'Memory card',
    affectedGames: [
      { installationId: 'id-fixture:3', gameId: '3', name: 'Card A' },
      { installationId: 'id-fixture:4', gameId: '4', name: 'Card B' },
    ],
    consent: 'pending', unavailable: false,
  },
  {
    pluginId: 'community.example', publisherFingerprint: publisher, profileId: 'profile-b',
    profileName: 'Shared memory', gameId: '5', installationId: 'id-fixture:5', runtimeId: 'org.dolphin',
    resourceId: 'shared-mem', displayName: 'Shared memory', affectedGames: [
      { installationId: 'id-fixture:5', gameId: '5', name: 'Card C' },
    ],
    consent: 'current', unavailable: false,
  },
  {
    pluginId: 'community.example', publisherFingerprint: publisher, profileId: 'profile-c',
    profileName: 'Lost card', gameId: '6', installationId: 'id-fixture:6', runtimeId: 'org.dolphin',
    resourceId: 'lost', displayName: 'Lost card', affectedGames: [],
    consent: 'stale', unavailable: true,
  },
];

function renderPanel(locale = 'en') {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <I18nProvider initialLocale={locale}>
          <PluginSharedResources pluginId="community.example" publisherFingerprint={publisher} />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('PluginSharedResources', () => {
  beforeEach(() => {
    vi.spyOn(commands, 'pluginSharedResourceReviews').mockResolvedValue(reviews);
    vi.spyOn(commands, 'pluginSharedResourceAccept').mockResolvedValue(undefined);
    vi.spyOn(commands, 'pluginSharedResourceRevoke').mockResolvedValue(true);
  });
  afterEach(() => vi.restoreAllMocks());

  it.each([
    ['en', 'Shared emulator resources', 'Approve use', 'Revoke', 'Card A, Card B'],
    ['pt-BR', 'Recursos de emulador compartilhados', 'Aprovar uso', 'Revogar', 'Card A, Card B'],
  ])('shows the exact affected scope and separate review controls in %s', async (locale, heading, accept, revoke, games) => {
    renderPanel(locale);
    expect(await screen.findByRole('region', { name: heading })).toBeInTheDocument();
    expect(await screen.findByText(new RegExp(games))).toBeInTheDocument();
    expect(screen.getByText(new RegExp(locale === 'en' ? 'adding a game|new review' : 'adicionar um jogo|nova revisão', 'i'))).toBeInTheDocument();
    expect(screen.getByRole('note')).toHaveTextContent(
      locale === 'en' ? 'cannot be resolved' : 'não pode ser resolvida',
    );
    const approveButtons = screen.getAllByRole('button', { name: accept });
    expect(approveButtons[0]).toBeEnabled();
    const revokeButtons = screen.getAllByRole('button', { name: revoke });
    expect(revokeButtons[1]).toBeEnabled();

    fireEvent.click(approveButtons[0]);
    await waitFor(() => expect(commands.pluginSharedResourceAccept).toHaveBeenCalledWith(
      'community.example', publisher, 'profile-a', 'memory-card',
    ));

    fireEvent.click(revokeButtons[1]);
    await waitFor(() => expect(commands.pluginSharedResourceRevoke).toHaveBeenCalledWith(
      'community.example', publisher, 'profile-b', 'shared-mem',
    ));
  });

  it('disables approval when the affected scope cannot be resolved', async () => {
    renderPanel();
    await screen.findByRole('note');
    const approveButtons = screen.getAllByRole('button', { name: 'Approve use' });
    expect(approveButtons[approveButtons.length - 1]).toBeDisabled();
  });
});
