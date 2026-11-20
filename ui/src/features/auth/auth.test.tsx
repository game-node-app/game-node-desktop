import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import React, { useState } from 'react';
import { commands } from '../../api/commands';
import type { AuthInput, AuthView, Notice } from '../../api/types';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { NoticeBanner, NOTICE_MSGID_MAP } from './NoticeBanner';
import { SignedOutView } from './SignedOutView';

function pending(overrides: Partial<{
  provider: 'google' | 'discord' | 'twitter';
  awaitingBrowser: boolean;
  canReopen: boolean;
  reopenFailed: boolean;
}> = {}): AuthView {
  return {
    pending: {
      provider: 'google',
      awaitingBrowser: true,
      canReopen: true,
      reopenFailed: false,
      ...overrides,
    },
  };
}

describe('features/auth', () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = createAppQueryClient();
    resetUiStore();
    vi.restoreAllMocks();
  });

  const renderWithProviders = (ui: React.ReactElement, locale = 'en') => {
    return render(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={queryClient}>
          <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    );
  };

  describe('NoticeBanner', () => {
    it('returns null when notice is not provided', () => {
      renderWithProviders(<NoticeBanner notice={null} />);
      expect(screen.queryByTestId('notice-banner')).toBeNull();
    });

    it('maps and translates all Notice variants in English', () => {
      const notices = Object.keys(NOTICE_MSGID_MAP) as Notice[];

      for (const notice of notices) {
        const { unmount } = renderWithProviders(<NoticeBanner notice={notice} />, 'en');
        const expectedMsg = NOTICE_MSGID_MAP[notice];
        expect(screen.getByTestId('notice-banner')).toHaveTextContent(expectedMsg);
        unmount();
      }
    });

    it('translates Notice variants in pt-BR', () => {
      const { unmount: u1 } = renderWithProviders(
        <NoticeBanner notice="signInCancelled" />,
        'pt-BR',
      );
      expect(screen.getByTestId('notice-banner')).toHaveTextContent('Entrada cancelada.');
      u1();

      const { unmount: u2 } = renderWithProviders(
        <NoticeBanner notice="browserLaunchFailed" />,
        'pt-BR',
      );
      expect(screen.getByTestId('notice-banner')).toHaveTextContent(
        'Não foi possível abrir o navegador. Tente novamente.',
      );
      u2();

      const { unmount: u3 } = renderWithProviders(
        <NoticeBanner notice="connectionProblem" />,
        'pt-BR',
      );
      expect(screen.getByTestId('notice-banner')).toHaveTextContent(
        'Não foi possível conectar. Sua sessão foi preservada; tente novamente quando estiver online.',
      );
      u3();
    });
  });

  describe('SignedOutView', () => {
    it('dispatches startSignIn on each provider button', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth="signedOut" />);

      fireEvent.click(screen.getByRole('button', { name: /continue with google/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith({ startSignIn: 'google' });

      fireEvent.click(screen.getByRole('button', { name: /continue with discord/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith({ startSignIn: 'discord' });

      fireEvent.click(screen.getByRole('button', { name: /continue with twitter/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith({ startSignIn: 'twitter' });
    });

    it('renders 25 real covers with cached files first and IGDB artwork filling the rest', async () => {
      vi.spyOn(commands, 'signInCoverWall').mockResolvedValue([
        '123.jpg',
        'local-game-0123456789abcdef.webp',
      ]);

      renderWithProviders(<SignedOutView auth="signedOut" />);

      const mural = screen.getByTestId('sign-in-cover-mural');
      expect(mural).toHaveAttribute('aria-hidden', 'true');

      const images = await screen.findAllByTestId('sign-in-cover-image');
      expect(images).toHaveLength(25);
      await waitFor(() =>
        expect(images[0]).toHaveAttribute(
          'src',
          'gamenode-asset://localhost/cover/123.jpg',
        ),
      );
      expect(images[1]).toHaveAttribute(
        'src',
        'gamenode-asset://localhost/cover/local-game-0123456789abcdef.webp',
      );
      for (const image of images) {
        expect(image).toHaveAttribute('loading', 'lazy');
        expect(image).toHaveAttribute('decoding', 'async');
        expect(image).toHaveAttribute('alt', '');
        expect(image.getAttribute('src')).not.toContain('mock-');
      }

      const tiles = screen.getAllByTestId('sign-in-cover-tile');
      expect(tiles).toHaveLength(25);
      expect(tiles.slice(0, 2).map((tile) => tile.dataset.source)).toEqual([
        'cache',
        'cache',
      ]);
      expect(tiles.filter((tile) => tile.dataset.source === 'curated')).toHaveLength(23);
      expect(images[2]?.getAttribute('src')).toContain('signin-covers');
      expect(screen.queryByText(/game\s*0?1/i)).toBeNull();
    });

    it('fills an empty cover wall with bundled IGDB artwork instead of neutral tiles', async () => {
      vi.spyOn(commands, 'signInCoverWall').mockResolvedValue([]);

      renderWithProviders(<SignedOutView auth="signedOut" />);

      const images = await screen.findAllByTestId('sign-in-cover-image');
      expect(images).toHaveLength(25);
      for (const image of images) {
        expect(image.getAttribute('src')).toContain('signin-covers');
      }
      expect(
        screen
          .getAllByTestId('sign-in-cover-tile')
          .every((tile) => tile.dataset.source === 'curated'),
      ).toBe(true);
    });

    it('keeps bundled IGDB artwork when signInCoverWall rejects and falls back if a cached cover fails to load', async () => {
      const spy = vi
        .spyOn(commands, 'signInCoverWall')
        .mockRejectedValueOnce(new Error('offline or unreadable cache'));

      const { unmount } = renderWithProviders(<SignedOutView auth="signedOut" />);
      const offlineImages = await screen.findAllByTestId('sign-in-cover-image');
      expect(offlineImages).toHaveLength(25);
      expect(
        offlineImages.every((img) => img.getAttribute('src')?.includes('signin-covers')),
      ).toBe(true);
      unmount();

      queryClient.clear();
      spy.mockResolvedValueOnce(['broken-cache.jpg']);
      renderWithProviders(<SignedOutView auth="signedOut" />);
      const images = await screen.findAllByTestId('sign-in-cover-image');
      await waitFor(() =>
        expect(images[0]).toHaveAttribute(
          'src',
          'gamenode-asset://localhost/cover/broken-cache.jpg',
        ),
      );

      fireEvent.error(images[0]!);
      await waitFor(() =>
        expect(screen.getAllByTestId('sign-in-cover-tile')[0]?.dataset.source).toBe(
          'curated',
        ),
      );
      expect(
        screen.getAllByTestId('sign-in-cover-image')[0]?.getAttribute('src'),
      ).toContain('signin-covers');
    });

    it('uses no "use without an account" entry', () => {
      renderWithProviders(<SignedOutView auth="signedOut" />);
      expect(screen.queryByText(/use without an account/i)).toBeNull();
    });

    it('handles the pending state and its actions', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth={pending()} />);

      expect(
        screen.getByText('Finish signing in in your browser.'),
      ).toBeInTheDocument();
      expect(
        screen.getByText(
          "We opened the GameNode page in your default browser. Come back here when you're done.",
        ),
      ).toBeInTheDocument();
      expect(screen.getByTestId('sign-in-pending-status')).toHaveTextContent(
        'Waiting for confirmation…',
      );

      const openAgain = screen.getByRole('button', { name: /open again/i });
      expect(openAgain).toBeEnabled();
      fireEvent.click(openAgain);
      expect(dispatchAuthSpy).toHaveBeenCalledWith('reopenBrowser');

      fireEvent.click(screen.getByRole('button', { name: /cancel sign-in/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith('cancelSignIn');
    });

    it('shows the confirming text and disables Open again until the address is known', () => {
      renderWithProviders(
        <SignedOutView
          auth={pending({ awaitingBrowser: false, canReopen: false })}
        />,
      );

      expect(screen.getByTestId('sign-in-pending-status')).toHaveTextContent(
        'Confirming your account…',
      );
      expect(screen.getByRole('button', { name: /open again/i })).toBeDisabled();
    });

    it('cancels on Escape', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth={pending()} />);
      fireEvent.keyDown(window, { key: 'Escape' });

      expect(dispatchAuthSpy).toHaveBeenCalledWith('cancelSignIn');
    });

    it('shows the localized reopen failure while the attempt keeps waiting', () => {
      renderWithProviders(
        <SignedOutView auth={pending({ reopenFailed: true })} />,
      );

      expect(screen.getByTestId('sign-in-reopen-failed')).toHaveTextContent(
        'Could not open your browser again. Please try again.',
      );
      expect(screen.getByTestId('sign-in-pending-status')).toHaveTextContent(
        'Waiting for confirmation…',
      );
      expect(screen.getByRole('button', { name: /open again/i })).toBeEnabled();
    });

    it('restores focus to the provider button after cancelling', async () => {
      const user = userEvent.setup();
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth');

      function Harness() {
        const [auth, setAuth] = useState<AuthView>('signedOut');
        dispatchAuthSpy.mockImplementation(async (input: AuthInput) => {
          if (typeof input === 'object' && 'startSignIn' in input) {
            setAuth(pending({ provider: input.startSignIn }));
          } else if (input === 'cancelSignIn') {
            setAuth('signedOut');
          }
        });
        return <SignedOutView auth={auth} />;
      }

      renderWithProviders(<Harness />);
      const google = screen.getByRole('button', { name: /continue with google/i });
      await user.click(google);
      expect(await screen.findByTestId('sign-in-pending')).toBeInTheDocument();

      await user.keyboard('{Escape}');
      await waitFor(() =>
        expect(screen.queryByTestId('sign-in-pending')).not.toBeInTheDocument(),
      );
      await waitFor(() => expect(document.activeElement).toBe(google));
    });

    it('renders verifying as a banner and keeps its cancel action', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth="verifying" />);

      expect(screen.getByText('Verifying sign-in…')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: /cancel sign-in/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith('cancelSignIn');
    });

    it('handles restoreFailed state with retry connection and sign out', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth="restoreFailed" />);

      fireEvent.click(screen.getByRole('button', { name: /retry connection/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith('retryRestore');

      fireEvent.click(screen.getByRole('button', { name: /sign out/i }));
      expect(dispatchAuthSpy).toHaveBeenCalledWith('signOut');
    });

    it('localizes the screen and buttons in pt-BR', async () => {
      renderWithProviders(
        <SignedOutView auth="signedOut" notice="attemptExpired" />,
        'pt-BR',
      );

      expect(screen.getByText('Boas-vindas ao GameNode')).toBeInTheDocument();
      expect(
        screen.getByText('Todas as suas lojas, um só lugar.'),
      ).toBeInTheDocument();
      expect(
        screen.getByText(
          'Steam, Epic Games Store e jogos DRM-free sincronizados com o seu perfil GameNode.',
        ),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Continuar com Google' }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Continuar com Discord' }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Continuar com Twitter' }),
      ).toBeInTheDocument();
      expect(
        screen.getByText('Entre com segurança pelo seu navegador padrão.'),
      ).toBeInTheDocument();
      expect(
        screen.getByText('A tentativa de entrada expirou. Tente novamente.'),
      ).toBeInTheDocument();
      expect(await screen.findByTestId('sign-in-version')).toHaveTextContent(
        'GameNode Desktop · versão 0.1.0',
      );
      expect(screen.queryByText(/usar sem conta/i)).toBeNull();
    });

    it('localizes the pending state in pt-BR', () => {
      renderWithProviders(
        <SignedOutView
          auth={pending({ provider: 'discord', canReopen: true })}
        />,
        'pt-BR',
      );

      expect(
        screen.getByText('Conclua a entrada no navegador.'),
      ).toBeInTheDocument();
      expect(screen.getByTestId('sign-in-pending-status')).toHaveTextContent(
        'Aguardando confirmação…',
      );
      expect(
        screen.getByRole('button', { name: 'Abrir de novo' }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Cancelar entrada' }),
      ).toBeInTheDocument();
    });
  });
});
