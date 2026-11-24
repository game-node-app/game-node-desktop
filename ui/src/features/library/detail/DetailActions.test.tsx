import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor } from '@testing-library/react';
import {
  commands,
  type Actions,
  type DetailView,
  type PluginInfo,
  type Row,
} from '../../../api';
import { renderControls } from '../controlsTestUtils';
import { DetailActions } from './DetailActions';

const row: Row = {
  key: 'steam:1',
  title: 'Game',
  detectedName: 'Game',
  matchedName: 'Game',
  provider: 'steam',
  providerName: 'Steam',
  installed: true,
  owned: true,
  shared: false,
  running: false,
  status: { matched: { gameId: 42, source: 'automatic' } },
  coverFile: null,
  playtime: { seconds: 0, source: 'store', lastPlayedAt: null },
  releaseYear: 2020,
  addedAt: 0,
  isFavorite: false,
  hidden: false,
  gamenode: null,
};

const detail: DetailView = {
  running: false,
  row,
  installDir: '/games/Game',
  releaseYear: 2020,
  hasArtwork: false,
  membership: 'notInLibrary',
  achievementBackend: false,
  achievements: null,
  suggestions: [],
};

const actions: Actions = {
  selected: 1,
  play: { disabled: 'noLaunchRule' },
  install: 'available',
  matchGame: 'available',
  addToGamenode: 'available',
  changeMatch: true,
};

const extension: PluginInfo = {
  providerId: 'extension:org.example.helper',
  pluginId: 'org.example.helper',
  pluginKind: 'extension',
  name: 'Helper',
  source: 'community',
  status: 'installed',
  capabilities: [],
  session: 'stopped',
  nativeAccess: [],
  publisherFingerprint: 'a'.repeat(64),
};

function renderActions() {
  return renderControls(
    <DetailActions
      detail={detail}
      actions={actions}
      installPath="/games/Game"
      isRunning={false}
      isDrmFree={false}
      inLibrary={false}
      gameId={42}
      tools={[]}
      onViewAchievements={() => {}}
    />,
  );
}

describe('DetailActions', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('offers a trusted extension when the store has no native launch', async () => {
    vi.spyOn(commands, 'availablePluginExtensions').mockResolvedValue([extension]);
    const start = vi.spyOn(commands, 'startExtensionAction').mockResolvedValue(undefined);
    renderActions();

    const button = await screen.findByTestId('detail-extension-play-btn');
    expect(button).toHaveTextContent('Start with extension · Helper');
    fireEvent.click(button);
    await waitFor(() =>
      expect(start).toHaveBeenCalledWith('steam:1', 'org.example.helper', 'play'),
    );
  });

  it('keeps the normal Play button when the store can launch', async () => {
    const available = vi
      .spyOn(commands, 'availablePluginExtensions')
      .mockResolvedValue([extension]);
    renderControls(
      <DetailActions
        detail={detail}
        actions={{ ...actions, play: 'available' }}
        installPath="/games/Game"
        isRunning={false}
        isDrmFree={false}
        inLibrary={false}
        gameId={42}
        tools={[]}
        onViewAchievements={() => {}}
      />,
    );

    expect(await screen.findByTestId('detail-play-btn')).toBeInTheDocument();
    expect(screen.queryByTestId('detail-extension-play-btn')).not.toBeInTheDocument();
    expect(available).not.toHaveBeenCalled();
  });
});
