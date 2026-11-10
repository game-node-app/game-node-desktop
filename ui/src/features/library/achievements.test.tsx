import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultFilters,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type AchievementListView,
  type AchievementRecord,
  type AchievementSummary,
  type Actions,
  type DetailView,
  type LibraryView,
  type Row,
  type Snapshot,
  type StoreAccountState,
  type StoreAccountView,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { DetailPane } from './DetailPane';

const actions: Actions = {
  selected: 1,
  play: 'available',
  install: 'available',
  matchGame: 'available',
  addToGamenode: 'available',
  changeMatch: false,
};

function row(overrides: Partial<Row> = {}): Row {
  return {
    key: 'steam:620',
    title: 'Portal 2',
    detectedName: 'Portal 2',
    matchedName: null,
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    familyShared: false,
    running: false,
    status: 'unmatched',
    coverFile: null,
    playtime: { seconds: null, source: 'store', lastPlayedAt: null },
    releaseYear: 2011,
    addedAt: 0,
    isFavorite: false,
    ...overrides,
  };
}

function detail(
  overrides: Partial<DetailView> = {},
): DetailView {
  return {
    row: row(),
    running: false,
    installDir: '/games/Portal 2',
    releaseYear: 2011,
    hasArtwork: false,
    membership: 'notInLibrary',
    achievementBackend: true,
    achievements: null,
    ...overrides,
  };
}

function library(overrides: Partial<LibraryView> = {}): LibraryView {
  const first = row();
  return {
    generation: 1,
    layout: {
      view: 'grid',
      sort: { key: 'title', direction: 'ascending' },
      filter: 'all',
      filters: createDefaultFilters(),
      sidebarWidth: 216,
      detailWidth: 320,
      detailOpen: true,
    },
    search: '',
    access: 'verified',
    total: 1,
    installedTotal: 1,
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: 1,
        installedCount: 1,
        ownedCount: 1,
        familyCount: 0,
      },
    ],
    rows: [first],
    scanning: false,
    failedProviders: [],
    selected: [first.key],
    focused: first.key,
    actions,
    detail: detail(),
    empty: null,
    ...overrides,
  };
}

function account(state: StoreAccountState): StoreAccountView {
  return {
    provider: 'steam',
    state,
    accountId: state === 'notConnected' ? null : '76561198000000000',
    accountName: state === 'notConnected' ? null : 'cassi',
    ownedCount: 1,
    familyCount: 0,
    installedCount: 1,
    lastCheckedAt: null,
    storage: 'secretService',
    persistFailed: false,
    catalogError: null,
    familyError: null,
    canSignIn: false,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: true,
    canCheck: true,
    canRemoveCredential: false,
  };
}

function summary(overrides: Partial<AchievementSummary> = {}): AchievementSummary {
  return {
    state: 'ready',
    unlocked: 2,
    total: 5,
    latestUnlock: {
      id: 'ACH_FIRST',
      name: 'First steps',
      unlockedAt: 1_700_000_000,
    },
    lastSuccessAt: 1_700_000_100,
    failure: null,
    ...overrides,
  };
}

function renderDetail(
  snapshot: Snapshot,
  locale = 'en',
): ReturnType<typeof render> {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  return render(
    <QueryClientProvider client={queryClient}>
      <MantineProvider>
        <I18nProvider initialLocale={locale}>
          <DetailPane />
        </I18nProvider>
      </MantineProvider>
    </QueryClientProvider>,
  );
}

function snapshotWith(
  state: 'connected' | 'notConnected' | 'expired',
  detailValue: DetailView,
): Snapshot {
  const snapshot = createDefaultSnapshot();
  snapshot.library = library({ detail: detailValue });
  snapshot.storeAccounts = [account(state)];
  return snapshot;
}

function achievement(id: string, overrides: Partial<AchievementRecord> = {}): AchievementRecord {
  return {
    id,
    name: `Achievement ${id}`,
    description: `Description ${id}`,
    hidden: false,
    iconUrl: '',
    iconGrayUrl: '',
    unlocked: false,
    unlockedAt: null,
    position: 0,
    ...overrides,
  };
}

function list(achievements: AchievementRecord[], state: 'ready' | 'none' | 'failed' = 'ready'): AchievementListView {
  return {
    connected: true,
    state,
    lastSuccessAt: 1_700_000_100,
    failure: state === 'failed' ? 'privacy' : null,
    achievements,
  };
}

describe('Achievement detail section', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('shows a ready summary with a non-color progress indication and the latest unlock', () => {
    renderDetail(snapshotWith('connected', detail({ achievements: summary() })));
    const section = screen.getByTestId('detail-achievements');
    expect(within(section).getByText('Achievements')).toBeInTheDocument();
    expect(within(section).getByText('2 of 5 unlocked')).toBeInTheDocument();
    const progress = within(section).getByRole('progressbar');
    expect(progress).toHaveAttribute('aria-valuenow', '2');
    expect(progress).toHaveAttribute('aria-valuemax', '5');
    expect(within(section).getByText('First steps')).toBeInTheDocument();
    expect(within(section).getByTestId('detail-achievement-updated')).toBeInTheDocument();
  });

  it('distinguishes a game with no achievements from loading and never shows a progress bar', () => {
    const none = renderDetail(
      snapshotWith('connected', detail({ achievements: summary({ state: 'none', unlocked: 0, total: 0, latestUnlock: null }) })),
    );
    expect(screen.getByTestId('detail-achievements-none')).toBeInTheDocument();
    expect(screen.queryByTestId('detail-achievement-progress')).toBeNull();
    none.unmount();

    renderDetail(snapshotWith('connected', detail({ achievements: null })));
    expect(screen.getByTestId('detail-achievements-loading')).toBeInTheDocument();
    expect(screen.queryByText('This game has no achievements.')).toBeNull();
  });

  it('explains privacy failures and offers the cached data', () => {
    renderDetail(
      snapshotWith(
        'connected',
        detail({ achievements: summary({ failure: 'privacy' }) }),
      ),
    );
    expect(
      screen.getByText('Steam privacy settings prevent reading these achievements.'),
    ).toBeInTheDocument();
    expect(screen.getByTestId('detail-achievement-progress')).toBeInTheDocument();
  });

  it('shows a failed state when nothing is cached', () => {
    renderDetail(
      snapshotWith(
        'connected',
        detail({ achievements: summary({ state: 'failed', unlocked: 0, total: 0, latestUnlock: null, lastSuccessAt: null, failure: 'unreachable' }) }),
      ),
    );
    expect(screen.getByTestId('detail-achievements-failed')).toBeInTheDocument();
    expect(screen.queryByTestId('detail-achievement-progress')).toBeNull();
  });

  it('asks for a connection when Steam is not connected or expired', () => {
    const disconnected = renderDetail(snapshotWith('notConnected', detail()));
    expect(screen.getByTestId('detail-achievements-not-connected')).toBeInTheDocument();
    expect(screen.getByText('Connect Steam to see achievements.')).toBeInTheDocument();
    disconnected.unmount();

    renderDetail(snapshotWith('expired', detail()));
    expect(screen.getByTestId('detail-achievements-expired')).toBeInTheDocument();
  });

  it('shows no section for a provider without an achievement backend', () => {
    renderDetail(
      snapshotWith(
        'connected',
        detail({ achievementBackend: false, achievements: summary() }),
      ),
    );
    expect(screen.queryByTestId('detail-achievements')).toBeNull();
  });

  it('localizes the section in Brazilian Portuguese', () => {
    renderDetail(snapshotWith('connected', detail({ achievements: summary() })), 'pt-BR');
    expect(screen.getByText('Conquistas')).toBeInTheDocument();
    expect(screen.getByText('2 de 5 desbloqueadas')).toBeInTheDocument();
  });
});

describe('Achievement list window', () => {
  const clientSize = (element: HTMLElement): number =>
    element.getAttribute('data-testid') === 'achievement-list-scroll' ? 480 : 0;
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
    // jsdom performs no layout; the virtualizer still needs a measurable viewport.
    Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
      configurable: true,
      get() {
        return clientSize(this as HTMLElement);
      },
    });
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get() {
        return clientSize(this as HTMLElement);
      },
    });
  });
  afterEach(() => {
    delete (HTMLElement.prototype as { clientHeight?: number }).clientHeight;
    delete (HTMLElement.prototype as { clientWidth?: number }).clientWidth;
  });

  it('opens by keyboard, filters, closes with Escape and restores focus', async () => {
    const records = [
      achievement('ACH_NEW', { unlocked: true, unlockedAt: 1_700_000_000 }),
      achievement('ACH_HIDDEN', { hidden: true }),
    ];
    const listSpy = vi
      .spyOn(commands, 'achievementList')
      .mockResolvedValue(list(records));
    renderDetail(snapshotWith('connected', detail({ achievements: summary() })));
    const user = userEvent.setup();
    const open = screen.getByTestId('detail-achievements-open');
    open.focus();
    await user.keyboard('{Enter}');
    expect(await screen.findByTestId('achievement-list-modal')).toBeInTheDocument();
    expect(listSpy).toHaveBeenCalledWith('steam:620', 'all');
    const ordered = (await screen.findAllByTestId('achievement-row')).map(
      (row) => row.getAttribute('data-achievement-id'),
    );
    expect(ordered).toEqual(['ACH_NEW', 'ACH_HIDDEN']);

    // The backend masks a locked hidden achievement's text; the UI supplies the generic label.
    expect(screen.getByText('Hidden achievement')).toBeInTheDocument();
    expect(screen.queryByText('A secret')).toBeNull();

    await user.click(screen.getByRole('radio', { name: 'Unlocked' }));
    await waitFor(() => expect(listSpy).toHaveBeenLastCalledWith('steam:620', 'unlocked'));

    await user.keyboard('{Escape}');
    await waitFor(() =>
      expect(screen.queryByTestId('achievement-list-modal')).toBeNull(),
    );
    expect(open).toHaveFocus();
  });

  it('renders only the visible rows for a game with hundreds of achievements', async () => {
    const records = Array.from({ length: 500 }, (_, index) =>
      achievement(`ACH_${index}`, {
        name: `Achievement ${index}`,
        unlocked: index < 250,
        unlockedAt: index < 250 ? 1_700_000_000 + index : null,
        position: index,
      }),
    );
    vi.spyOn(commands, 'achievementList').mockResolvedValue(list(records));
    renderDetail(snapshotWith('connected', detail({ achievements: summary() })));
    fireEvent.click(screen.getByTestId('detail-achievements-open'));
    const rows = await screen.findAllByTestId('achievement-row');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(100);
  });

  it('shows no date for an unlocked achievement without an unlock time', async () => {
    vi.spyOn(commands, 'achievementList').mockResolvedValue(
      list([achievement('ACH_NO_TIME', { unlocked: true, unlockedAt: null, name: 'No time' })]),
    );
    renderDetail(snapshotWith('connected', detail({ achievements: summary() })));
    fireEvent.click(screen.getByTestId('detail-achievements-open'));
    const row = await screen.findByTestId('achievement-row');
    expect(within(row).getByText('No time')).toBeInTheDocument();
    expect(within(row).queryByText(/\b(19|20)\d{2}\b/)).toBeNull();
  });

  it('renders an achievement icon through the asset protocol and hides a failed image', async () => {
    const iconUrl = `gamenode-asset://localhost/achievement/${'a'.repeat(64)}`;
    vi.spyOn(commands, 'achievementList').mockResolvedValue(
      list([achievement('ACH_ICON', { unlocked: true, iconUrl })]),
    );
    renderDetail(snapshotWith('connected', detail({ achievements: summary() })));
    fireEvent.click(screen.getByTestId('detail-achievements-open'));
    const row = await screen.findByTestId('achievement-row');
    const image = row.querySelector('img');
    expect(image).not.toBeNull();
    expect(image).toHaveAttribute('src', iconUrl);

    // The host downloads an icon after returning the list, so a failed request retries and
    // shows a placeholder in the meantime instead of a broken-image glyph.
    fireEvent.error(image!);
    expect(row.querySelector('img')).toBeNull();
    expect(row.querySelector('svg')).not.toBeNull();
  });
});
