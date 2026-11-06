import { beforeEach, describe, expect, it, vi } from 'vitest';
import { QueryClientProvider, type QueryClient } from '@tanstack/react-query';
import { createAppQueryClient } from '../../AppProviders';
import { render, screen, fireEvent } from '@testing-library/react';
import React from 'react';
import { commands } from '../../api/commands';
import type { Notice } from '../../api/types';
import { I18nProvider } from '../../i18n';
import { resetUiStore } from '../../store/uiStore';
import { NoticeBanner, NOTICE_MSGID_MAP } from './NoticeBanner';
import { SignedOutView } from './SignedOutView';

describe('features/auth', () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    queryClient = createAppQueryClient();
    resetUiStore();
    vi.restoreAllMocks();
  });

  const renderWithProviders = (ui: React.ReactElement, locale = 'en') => {
    return render(
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>,
    );
  };

  describe('NoticeBanner', () => {
    it('returns null when notice is not provided', () => {
      const { container } = renderWithProviders(<NoticeBanner notice={null} />);
      expect(container.firstChild).toBeNull();
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
    it('dispatches startSignIn on button clicks in default state', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth="signedOut" />);

      const googleBtn = screen.getByRole('button', { name: /continue with google/i });
      const discordBtn = screen.getByRole('button', { name: /continue with discord/i });
      const twitterBtn = screen.getByRole('button', { name: /continue with twitter/i });

      fireEvent.click(googleBtn);
      expect(dispatchAuthSpy).toHaveBeenCalledWith({ startSignIn: 'google' });

      fireEvent.click(discordBtn);
      expect(dispatchAuthSpy).toHaveBeenCalledWith({ startSignIn: 'discord' });

      fireEvent.click(twitterBtn);
      expect(dispatchAuthSpy).toHaveBeenCalledWith({ startSignIn: 'twitter' });
    });

    it('handles busy pending state and cancelSignIn action', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(
        <SignedOutView
          auth={{
            pending: {
              provider: 'google',
              awaitingBrowser: true,
            },
          }}
        />,
      );

      expect(
        screen.getByText('Complete sign-in in your browser.'),
      ).toBeInTheDocument();

      const cancelBtn = screen.getByRole('button', { name: /cancel sign-in/i });
      fireEvent.click(cancelBtn);
      expect(dispatchAuthSpy).toHaveBeenCalledWith('cancelSignIn');
    });

    it('handles restoreFailed state with retry connection and sign out', () => {
      const dispatchAuthSpy = vi.spyOn(commands, 'dispatchAuth').mockResolvedValue();

      renderWithProviders(<SignedOutView auth="restoreFailed" />);

      const retryBtn = screen.getByRole('button', { name: /retry connection/i });
      fireEvent.click(retryBtn);
      expect(dispatchAuthSpy).toHaveBeenCalledWith('retryRestore');

      const signOutBtn = screen.getByRole('button', { name: /sign out/i });
      fireEvent.click(signOutBtn);
      expect(dispatchAuthSpy).toHaveBeenCalledWith('signOut');
    });

    it('localizes texts and buttons in pt-BR', () => {
      renderWithProviders(
        <SignedOutView
          auth="signedOut"
          notice="attemptExpired"
        />,
        'pt-BR',
      );

      expect(screen.getByText('Boas-vindas ao GameNode')).toBeInTheDocument();
      expect(screen.getByText('Sinta-se em casa.')).toBeInTheDocument();
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
        screen.getByText('A tentativa de entrada expirou. Tente novamente.'),
      ).toBeInTheDocument();
    });

    it('localizes busy state in pt-BR', () => {
      renderWithProviders(
        <SignedOutView
          auth={{
            pending: {
              provider: 'discord',
              awaitingBrowser: true,
            },
          }}
        />,
        'pt-BR',
      );

      expect(
        screen.getByText('Conclua a entrada no navegador.'),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Cancelar entrada' }),
      ).toBeInTheDocument();
    });
  });
});
