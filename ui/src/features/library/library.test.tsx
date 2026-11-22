import React from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultLibraryView,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
  type Row,
  createDefaultFilters,
} from '../../api';
import { I18nProvider, useI18n } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import {
  CoverImage,
  resetLoadedCovers,
  GameGrid,
  GameList,
  DetailPane,
  formatReason,
  formatStatus,
  formatDuration,
  formatI18n,
} from './index';

function createMockRows(count: number): Row[] {
  return Array.from({ length: count }, (_, i) => ({
    key: `game-${i}`,
    title: `Game Title ${i}`,
    detectedName: `Detected Game ${i}`,
    matchedName: `GameNode Game ${i}`,
    provider: 'steam',
    providerName: 'Steam',
    installed: true,
    owned: true,
    shared: false,
    running: i === 0,
    sessionStartedAt: null,
    status:
      i % 3 === 0
        ? { matched: { gameId: 1000 + i, source: 'automatic' as const } }
        : i % 3 === 1
          ? ('ambiguous' as const)
          : ('unmatched' as const),
    coverFile: i % 2 === 0 ? `cover-${i}.jpg` : null,
    playtime: {
      seconds: i * 3600 + i * 60 + 45,
      source: i % 2 === 0 ? ('store' as const) : ('internal' as const),
      lastPlayedAt: 1_700_000_000 + i,
    },
    releaseYear: 2000 + i,
    addedAt: 1_700_000_000,
    isFavorite: i === 0,
    hidden: false,
    gamenode: null,
  }));
}

function makeLibrary(rows: Row[], overrides: Partial<ReturnType<typeof createDefaultLibraryView>> = {}) {
  return {
    ...createDefaultLibraryView(),
    generation: 1,
    layout: {
      ...createDefaultLibraryView().layout,
      filters: createDefaultFilters(),
      sidebarWidth: 240,
      detailWidth: 360,
      detailOpen: false,
    },
    search: '',
    access: 'verified' as const,
    total: rows.length,
    installedTotal: rows.filter((row) => row.installed).length,
    providers: [
      {
        id: 'steam',
        name: 'Steam',
        count: rows.length,
        installedCount: rows.filter((row) => row.installed).length,
        ownedCount: rows.length,
        sharedCount: 0,
        sharedLabel: 'Steam Family',
      },
    ],
    rows,
    scanning: false,
    failedProviders: [],
    selected: rows.length ? [rows[0].key] : [],
    focused: rows.length ? rows[0].key : null,
    actions: {
      selected: rows.length ? 1 : 0,
      play: 'available' as const,
      install: 'available' as const,
      matchGame: 'available' as const,
      addToGamenode: 'available' as const,
      changeMatch: false,
    },
    ...overrides,
  };
}

function renderWithProviders(ui: React.ReactElement, snapshot: Snapshot, locale = 'en') {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('Library Feature Tests', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('CoverImage', () => {
    beforeEach(() => {
      resetLoadedCovers();
    });

    it('retries after a failed cover when the cover file changes', () => {
      const { rerender } = render(<CoverImage coverFile="missing.jpg" title="Game" />);
      fireEvent.error(screen.getByRole('img'));
      expect(screen.getByTestId('cover-placeholder')).toBeInTheDocument();

      rerender(<CoverImage coverFile="arrived.jpg" title="Game" />);
      expect(screen.queryByTestId('cover-placeholder')).not.toBeInTheDocument();
      expect(screen.getByRole('img')).toHaveAttribute(
        'src',
        'gamenode-asset://localhost/cover/arrived.jpg',
      );
    });

    it('fades in on first load and renders known covers immediately on remount', () => {
      const first = render(<CoverImage coverFile="seen.jpg" title="Seen" />);
      expect(screen.getByRole('img')).toHaveStyle({ opacity: '0' });
      fireEvent.load(screen.getByRole('img'));
      expect(screen.getByRole('img')).toHaveStyle({ opacity: '1' });
      first.unmount();

      render(<CoverImage coverFile="seen.jpg" title="Seen" />);
      expect(screen.getByRole('img')).toHaveStyle({ opacity: '1' });
    });

    it('renders img with the asset protocol and lazy async decoding', () => {
      render(
        <CoverImage
          coverFile="witcher3.jpg"
          title="The Witcher 3"
          detectedName="witcher3_exe"
        />,
      );

      const img = screen.getByRole('img');
      expect(img).toHaveAttribute('src', 'gamenode-asset://localhost/cover/witcher3.jpg');
      expect(img).toHaveAttribute('alt', 'The Witcher 3');
      expect(img).toHaveAttribute('loading', 'lazy');
      expect(img).toHaveAttribute('decoding', 'async');
    });

    it('renders the styled placeholder with title and detected name', () => {
      const { rerender } = render(
        <CoverImage
          coverFile={null}
          title="Cyberpunk 2077"
          detectedName="cyberpunk_pkg"
        />,
      );

      const placeholder = screen.getByTestId('cover-placeholder');
      expect(placeholder).toHaveTextContent('Cyberpunk 2077');
      expect(placeholder).toHaveTextContent('cyberpunk_pkg');
      expect(screen.queryByRole('img')).not.toBeInTheDocument();

      rerender(
        <CoverImage
          coverFile="bad-image.jpg"
          title="Error Game"
          detectedName="error_pkg"
        />,
      );
      fireEvent.error(screen.getByRole('img'));
      expect(screen.getByTestId('cover-placeholder')).toHaveTextContent('Error Game');
    });
  });

  describe('Virtualized GameGrid and GameList with 500-item fixture', () => {
    it('mounts only viewport rows in GameGrid for 500 items', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary(createMockRows(500));

      renderWithProviders(<GameGrid />, snapshot);

      const renderedTiles = screen.getAllByRole('listitem');
      expect(renderedTiles.length).toBeLessThan(60);
      expect(renderedTiles.length).toBeGreaterThan(0);

      const imgs = screen.getAllByRole('img');
      expect(imgs.length).toBeGreaterThan(0);
      expect(imgs[0].getAttribute('src')).toMatch(
        /^gamenode-asset:\/\/localhost\/cover\//,
      );
      expect(imgs[0]).toHaveAttribute('loading', 'lazy');
      expect(imgs[0]).toHaveAttribute('decoding', 'async');
    });

    it('renders records from a third-party provider in the grid', () => {
      const snapshot = createDefaultSnapshot();
      const row = {
        ...createMockRows(1)[0],
        key: 'dolphin-emu:GZLE01',
        title: 'The Legend of Zelda',
        detectedName: 'The Legend of Zelda',
        provider: 'dolphin-emu',
        providerName: 'Dolphin GameCube and Wii',
      };
      snapshot.library = makeLibrary([row], {
        providers: [
          {
            id: 'dolphin-emu',
            name: 'Dolphin GameCube and Wii',
            count: 1,
            installedCount: 1,
            ownedCount: 1,
            sharedCount: 0,
            sharedLabel: null,
          },
        ],
      });

      renderWithProviders(<GameGrid />, snapshot);

      expect(screen.getByText('The Legend of Zelda')).toBeInTheDocument();
    });

    it('mounts only viewport rows in GameList for 500 items', () => {
      const snapshot = createDefaultSnapshot();
      snapshot.library = makeLibrary(createMockRows(500), {
        layout: { ...createDefaultLibraryView().layout, view: 'list' },
      });

      renderWithProviders(<GameList />, snapshot);

      const renderedRows = screen.getAllByRole('listitem');
      expect(renderedRows.length).toBeLessThan(60);
      expect(renderedRows.length).toBeGreaterThan(0);
      expect(screen.getByTestId('game-list-header')).toBeInTheDocument();
    });
  });

  describe('DetailPane disabled reasons and launch status', () => {
    it('displays localized disabled reasons in en and pt-BR', () => {
      const makeSnapshotWithReasons = (): Snapshot => {
        const snap = createDefaultSnapshot();
        const row = {
          ...createMockRows(1)[0],
          running: false,
          sessionStartedAt: null,
        };
        snap.library = makeLibrary([row], {
          layout: {
            ...createDefaultLibraryView().layout,
            filters: createDefaultFilters(),
            detailOpen: true,
          },
          detail: {
            running: false,
            row,
            installDir: '/games/hades',
            releaseYear: 2020,
            hasArtwork: true,
            membership: 'unavailable',
            achievementBackend: false,
            achievements: null,
            suggestions: [],
          },
          actions: {
            selected: 1,
            play: { disabled: 'needsVerifiedSession' },
            install: { disabled: 'noInstallRule' },
            matchGame: { disabled: 'notMatched' },
            addToGamenode: { disabled: { alreadyInLibrary: 'playing' } },
            changeMatch: true,
          },
        });
        return snap;
      };

      const { unmount } = renderWithProviders(<DetailPane />, makeSnapshotWithReasons(), 'en');
      expect(screen.getByTestId('detail-play-reason')).toHaveTextContent(
        'Your GameNode session is not verified yet. Retry the connection.',
      );
      expect(screen.getByTestId('detail-match-reason')).toHaveTextContent(
        'Match this game to a GameNode game first.',
      );
      expect(screen.getByTestId('detail-add-reason')).toHaveTextContent(
        'Already in your GameNode library as Playing.',
      );
      unmount();

      renderWithProviders(<DetailPane />, makeSnapshotWithReasons(), 'pt-BR');
      expect(screen.getByTestId('detail-play-reason')).toHaveTextContent(
        'Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.',
      );
      expect(screen.getByTestId('detail-match-reason')).toHaveTextContent(
        'Primeiro associe este jogo a um jogo do GameNode.',
      );
      expect(screen.getByTestId('detail-add-reason')).toHaveTextContent(
        'Já está na sua biblioteca do GameNode como Jogando.',
      );
    });

    it('renders launch status and allows dismiss', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissLaunch').mockResolvedValue();
      const snapshot = createDefaultSnapshot();
      const row = {
        ...createMockRows(1)[0],
        running: false,
        sessionStartedAt: null,
      };
      snapshot.launch = {
        entryKey: 'game-0',
        kind: 'play',
        state: { failed: 'noHandler' },
      };
      snapshot.library = makeLibrary([row], {
        layout: {
          ...createDefaultLibraryView().layout,
          filters: createDefaultFilters(),
          detailOpen: true,
        },
        detail: {
          running: false,
          row,
          installDir: '/games/hades',
          releaseYear: 2020,
          hasArtwork: true,
          membership: 'checking',
          achievementBackend: false,
          achievements: null,
          suggestions: [],
        },
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      });

      renderWithProviders(<DetailPane />, snapshot, 'en');

      expect(screen.getByTestId('detail-launch-status')).toHaveTextContent(
        'The store application is not installed or cannot be opened.',
      );
      fireEvent.click(screen.getByTestId('detail-dismiss-launch-btn'));
      expect(dismissSpy).toHaveBeenCalled();
    });
  });

  describe('Library Formatters and Localized Helpers', () => {
    it('formats entry status, duration, reason and interpolation in en and pt-BR', () => {
      let tEn!: (key: string) => string;
      let tPt!: (key: string) => string;

      const ConsumerEn = () => {
        const { t } = useI18n();
        tEn = t;
        return null;
      };
      const ConsumerPt = () => {
        const { t } = useI18n();
        tPt = t;
        return null;
      };

      const { unmount } = render(
        <I18nProvider initialLocale="en">
          <ConsumerEn />
        </I18nProvider>,
      );

      expect(formatStatus('playing', tEn)).toBe('Playing');
      expect(formatStatus('finished', tEn)).toBe('Finished');
      expect(formatStatus('planned', tEn)).toBe('Planned');
      expect(formatStatus('dropped', tEn)).toBe('Dropped');
      expect(formatStatus('ongoing', tEn)).toBe('Ongoing');
      expect(formatStatus(null, tEn)).toBe('');

      expect(formatDuration(null, tEn)).toBe('—');
      expect(formatDuration(-1, tEn)).toBe('—');
      expect(formatDuration(0, tEn)).toBe('0 min');
      expect(formatDuration(45, tEn)).toBe('45 s');
      expect(formatDuration(180, tEn)).toBe('3 min');
      expect(formatDuration(7260, tEn)).toBe('2 h 1 min');

      expect(formatI18n('Saved {} games', 5)).toBe('Saved 5 games');
      expect(formatI18n('Saved {} of {}', 2, 10)).toBe('Saved 2 of 10');
      expect(formatI18n('{0} from {1}', 'Hades', 'Steam')).toBe('Hades from Steam');

      expect(formatReason('running', tEn)).toBe('Running');
      expect(formatReason('nothingSelected', tEn)).toBe('Select a game first.');
      expect(formatReason('multipleSelected', tEn)).toBe('Select a single game.');
      expect(formatReason('noLaunchRule', tEn)).toBe(
        'This store cannot start games from GameNode.',
      );
      expect(formatReason('noInstallRule', tEn)).toBe(
        'This store cannot install games from GameNode.',
      );
      expect(formatReason('missingField', tEn)).toBe(
        'Details this action needs are missing. Rescan to refresh them.',
      );
      expect(formatReason('needsVerifiedSession', tEn)).toBe(
        'Your GameNode session is not verified yet. Retry the connection.',
      );
      expect(formatReason('offline', tEn)).toBe(
        'GameNode cannot be reached right now.',
      );
      expect(formatReason('notMatched', tEn)).toBe(
        'Match this game to a GameNode game first.',
      );
      expect(formatReason('noMatchedGames', tEn)).toBe(
        'None of the selected games is matched to a GameNode game.',
      );
      expect(formatReason('checkingMembership', tEn)).toBe(
        'Checking your GameNode library…',
      );
      expect(formatReason('membershipUnavailable', tEn)).toBe(
        'Could not check your GameNode library. Try again.',
      );
      expect(formatReason({ alreadyInLibrary: null }, tEn)).toBe(
        'Already in your GameNode library.',
      );
      expect(formatReason({ alreadyInLibrary: 'playing' }, tEn)).toBe(
        'Already in your GameNode library as Playing.',
      );
      expect(formatReason(null, tEn)).toBe('');

      unmount();

      render(
        <I18nProvider initialLocale="pt-BR">
          <ConsumerPt />
        </I18nProvider>,
      );

      expect(formatStatus('playing', tPt)).toBe('Jogando');
      expect(formatStatus('finished', tPt)).toBe('Zerado');
      expect(formatStatus('planned', tPt)).toBe('Na lista');
      expect(formatStatus('dropped', tPt)).toBe('Abandonado');
      expect(formatStatus('ongoing', tPt)).toBe('Em andamento');

      expect(formatReason('running', tPt)).toBe('Em execução');
      expect(formatReason('nothingSelected', tPt)).toBe('Selecione um jogo primeiro.');
      expect(formatReason('multipleSelected', tPt)).toBe('Selecione apenas um jogo.');
      expect(formatReason('noLaunchRule', tPt)).toBe(
        'Esta loja não pode iniciar jogos pelo GameNode.',
      );
      expect(formatReason('noInstallRule', tPt)).toBe(
        'Esta loja não pode instalar jogos pelo GameNode.',
      );
      expect(formatReason('missingField', tPt)).toBe(
        'Faltam detalhes de que esta ação precisa. Faça uma nova varredura para atualizá-los.',
      );
      expect(formatReason('needsVerifiedSession', tPt)).toBe(
        'Sua sessão do GameNode ainda não foi verificada. Tente conectar novamente.',
      );
      expect(formatReason('offline', tPt)).toBe(
        'Não foi possível acessar o GameNode agora.',
      );
      expect(formatReason('notMatched', tPt)).toBe(
        'Primeiro associe este jogo a um jogo do GameNode.',
      );
      expect(formatReason('noMatchedGames', tPt)).toBe(
        'Nenhum dos jogos selecionados está associado a um jogo do GameNode.',
      );
      expect(formatReason('checkingMembership', tPt)).toBe(
        'Verificando sua biblioteca do GameNode…',
      );
      expect(formatReason('membershipUnavailable', tPt)).toBe(
        'Não foi possível verificar sua biblioteca do GameNode. Tente novamente.',
      );
      expect(formatReason({ alreadyInLibrary: null }, tPt)).toBe(
        'Já está na sua biblioteca do GameNode.',
      );
      expect(formatReason({ alreadyInLibrary: 'playing' }, tPt)).toBe(
        'Já está na sua biblioteca do GameNode como Jogando.',
      );
    });
  });
});
