import { afterEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider } from '@tanstack/react-query';
import { act, render, screen } from '@testing-library/react';
import { createAppQueryClient } from '../../AppProviders';
import {
  createDefaultLibraryView,
  createDefaultSnapshot,
  type Row,
  type Snapshot,
  type StoreAccountView,
  type StoreAccountState,
} from '../../api/types';
import type { LibraryView } from '../../api/types';
import { I18nProvider } from '../../i18n';
import { formatSessionTime } from '../../hooks/useSessionClock';
import { StatusBar } from './StatusBar';

const NOW = 1_760_000_000;

function account(
  provider: string,
  state: StoreAccountState,
  overrides: Partial<StoreAccountView> = {},
): StoreAccountView {
  return {
    provider,
    state,
    accountId: 'account-1',
    accountName: 'cassi',
    ownedCount: 10,
    sharedCount: 0,
    sharedLabel: null,
    installedCount: 3,
    lastCheckedAt: NOW - 300,
    lastSyncedAt: null,
    storage: 'privateFile',
    persistFailed: false,
    catalogError: null,
    sharedError: null,
    canSignIn: false,
    canCancelSignIn: false,
    canFocusSignIn: false,
    canSignOut: true,
    canCheck: true,
    canRemoveCredential: false,
    ...overrides,
  };
}

function row(overrides: Partial<Row> = {}): Row {
  return {
    key: 'steam:620',
    title: 'Distant Skies: Recompiled',
    detectedName: 'Distant Skies: Recompiled',
    matchedName: 'Distant Skies: Recompiled',
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: false,
    sessionStartedAt: null,
    status: { matched: { gameId: 620, source: 'automatic' } },
    coverFile: null,
    playtime: { seconds: 3600, source: 'store', lastPlayedAt: null },
    releaseYear: 2024,
    addedAt: NOW - 86_400,
    isFavorite: false,
    hidden: false,
    gamenode: null,
    ...overrides,
  };
}

function library(overrides: Partial<LibraryView> = {}): LibraryView {  return {
    ...createDefaultLibraryView(),
    access: 'verified',
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: 10,
        installedCount: 3,
        ownedCount: 10,
        sharedCount: 0,
        sharedLabel: null,
      },
      {
        id: 'epic',
        name: 'Epic Games Store',
        count: 5,
        installedCount: 1,
        ownedCount: 5,
        sharedCount: 0,
        sharedLabel: null,
      },
      {
        id: 'gog',
        name: 'GOG',
        count: 2,
        installedCount: 1,
        ownedCount: 2,
        sharedCount: 0,
        sharedLabel: null,
      },
    ],
    ...overrides,
  };
}

function snapshot(overrides: Partial<Snapshot> = {}): Snapshot {
  const base = createDefaultSnapshot();
  return {
    ...base,
    library: library(),
    ...overrides,
  };
}

function renderStatusBar(
  data: Snapshot,
  props: {
    locale?: 'en' | 'pt-BR';
    now?: number;
    runningSessionSeconds?: number | null;
  } = {},
) {
  return render(
    <QueryClientProvider client={createAppQueryClient()}>
      <I18nProvider initialLocale={props.locale ?? 'en'}>
        <StatusBar
          snapshot={data}
          now={props.now}
          runningSessionSeconds={props.runningSessionSeconds}
        />
      </I18nProvider>
    </QueryClientProvider>,
  );
}

describe('features/shell/StatusBar', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('shows every connected store with an icon and text', () => {
    renderStatusBar(
      snapshot({
        storeAccounts: [
          account('steam', 'connected', { lastSyncedAt: NOW - 120 }),
          account('epic', 'expired'),
          account('gog', 'unreachable'),
          account('amazon', 'checking'),
          account('itch', 'notConnected'),
        ],
      }),
      { now: NOW },
    );

    expect(screen.getByTestId('status-store-steam')).toHaveTextContent(
      'Steam · synced 2 min ago',
    );
    expect(screen.getByTestId('status-store-epic')).toHaveTextContent(
      'Epic Games Store · session expired',
    );
    expect(screen.getByTestId('status-store-gog')).toHaveTextContent(
      'GOG · unreachable',
    );
    expect(screen.getByTestId('status-store-amazon')).toHaveTextContent(
      'Amazon · checking…',
    );
    expect(screen.queryByTestId('status-store-itch')).not.toBeInTheDocument();
  });

  it.each(['expired', 'invalidCredential', 'invalidCookies'] as const)(
    'shows a connected store with catalog error %s as expired',
    (catalogError) => {
      renderStatusBar(
        snapshot({
          storeAccounts: [account('steam', 'connected', {
            catalogError,
            lastSyncedAt: NOW - 120,
          })],
        }),
        { now: NOW },
      );

      expect(screen.getByTestId('status-store-steam')).toHaveTextContent(
        'Steam · session expired',
      );
    },
  );

  it('says when a connected store has not synced yet', () => {
    renderStatusBar(
      snapshot({ storeAccounts: [account('steam', 'connected')] }),
      { now: NOW },
    );
    expect(screen.getByTestId('status-store-steam')).toHaveTextContent(
      'Steam · not synced yet',
    );
  });

  it('shows the watched-folder last check as a relative time', () => {
    const data = snapshot({
      localGames: {
        ...createDefaultSnapshot().localGames,
        discoveryLastCheckedAt: NOW - 720,
      },
    });

    renderStatusBar(data, { now: NOW });

    expect(screen.getByTestId('status-watched-folders')).toHaveTextContent(
      'Watched folders · last check 12 min ago',
    );
  });

  it('shows watched folders as checking while discovery runs', () => {
    const data = snapshot({
      localGames: {
        ...createDefaultSnapshot().localGames,
        discoveryScanning: true,
        discoveryLastCheckedAt: NOW - 900,
      },
    });

    renderStatusBar(data, { now: NOW });

    expect(screen.getByTestId('status-watched-folders')).toHaveTextContent(
      'Watched folders · checking…',
    );
  });

  it('shows a hidden running game from the tracking snapshot with its session time', () => {
    const data = snapshot({
      tracking: {
        title: 'Distant Skies: Recompiled',
        available: true,
        running: ['steam:620'],
        sessionStartedAt: { 'steam:620': NOW - 2530 },
      },
      library: library({ rows: [] }),
    });

    renderStatusBar(data, { now: NOW });

    expect(screen.getByTestId('status-running')).toHaveTextContent(
      'Running: Distant Skies: Recompiled · 0:42:10',
    );
  });

  it('uses the process start from the snapshot and advances it every minute', () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW * 1000);
    const data = snapshot({
      tracking: {
        title: 'Distant Skies: Recompiled',
        available: true,
        running: ['steam:620'],
        sessionStartedAt: { 'steam:620': NOW - 2530 },
      },
      library: library({ rows: [] }),
    });

    renderStatusBar(data);

    expect(screen.getByTestId('status-running')).toHaveTextContent('0:42:10');
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(screen.getByTestId('status-running')).toHaveTextContent('0:43:10');
  });

  it('falls back to the row start and never invents a zero session', () => {
    const runningRow = row({
      running: true,
      sessionStartedAt: NOW - 125,
    });
    const data = snapshot({
      tracking: {
        title: '',
        available: true,
        running: [],
        sessionStartedAt: {},
      },
      library: library({ rows: [runningRow] }),
    });

    renderStatusBar(data, { now: NOW });
    expect(screen.getByTestId('status-running')).toHaveTextContent('0:02:05');
  });

  it('shows an unknown session instead of a fabricated zero', () => {
    const data = snapshot({
      tracking: {
        title: 'Distant Skies: Recompiled',
        available: true,
        running: ['steam:620'],
        sessionStartedAt: {},
      },
      library: library({ rows: [] }),
    });

    renderStatusBar(data, { now: NOW });
    expect(screen.getByTestId('status-running')).toHaveTextContent(
      'Running: Distant Skies: Recompiled · Unknown',
    );
    expect(screen.getByTestId('status-running')).not.toHaveTextContent('0:00:00');
  });

  it('refreshes relative times on the 60 s interval', () => {
    vi.useFakeTimers();
    vi.setSystemTime(NOW * 1000);

    const data = snapshot({
      storeAccounts: [
        account('steam', 'connected', { lastSyncedAt: NOW - 120 }),
      ],
      localGames: {
        ...createDefaultSnapshot().localGames,
        discoveryLastCheckedAt: NOW - 720,
      },
    });

    renderStatusBar(data);

    expect(screen.getByTestId('status-store-steam')).toHaveTextContent(
      'Steam · synced 2 min ago',
    );
    expect(screen.getByTestId('status-watched-folders')).toHaveTextContent(
      'last check 12 min ago',
    );

    act(() => {
      vi.advanceTimersByTime(60_000);
    });

    expect(screen.getByTestId('status-store-steam')).toHaveTextContent(
      'Steam · synced 3 min ago',
    );
    expect(screen.getByTestId('status-watched-folders')).toHaveTextContent(
      'last check 13 min ago',
    );
  });

  it('formats session times as h:mm:ss', () => {
    expect(formatSessionTime(0)).toBe('0:00:00');
    expect(formatSessionTime(2530)).toBe('0:42:10');
    expect(formatSessionTime(3 * 3600 + 5 * 60 + 9)).toBe('3:05:09');
  });

  it('localizes store states and the watched folders', () => {
    const data = snapshot({
      storeAccounts: [
        account('steam', 'connected', { lastSyncedAt: NOW - 120 }),
        account('epic', 'expired'),
      ],
      localGames: {
        ...createDefaultSnapshot().localGames,
        discoveryLastCheckedAt: NOW - 720,
      },
    });

    renderStatusBar(data, { locale: 'pt-BR', now: NOW });

    expect(screen.getByTestId('status-store-steam')).toHaveTextContent(
      'Steam · sincronizado há 2 min',
    );
    expect(screen.getByTestId('status-store-epic')).toHaveTextContent(
      'Epic Games Store · sessão expirada',
    );
    expect(screen.getByTestId('status-watched-folders')).toHaveTextContent(
      'Pastas monitoradas · última verificação há 12 min',
    );
  });
});
