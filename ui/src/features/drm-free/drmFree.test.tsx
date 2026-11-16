import { describe, expect, it, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultLibraryView,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Candidate,
  type PendingClaim,
  type Registration,
  type SaveProfile,
  type Snapshot,
  createDefaultFilters,
} from '../../api';
import { uiStore, resetUiStore } from '../../store/uiStore';
import { I18nProvider } from '../../i18n';
import {
  LocalGameEditor,
  DiscoveryModal,
  ClaimConflictModal,
  RemoveLocalGameModal,
  SaveProtectionModal,
} from './index';

function renderWithProviders(
  ui: React.ReactElement,
  initialSnapshot?: Snapshot,
  locale: 'en' | 'pt-BR' = 'en',
) {
  const queryClient = createAppQueryClient();
  const snap = initialSnapshot ?? createDefaultSnapshot();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snap);

  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark" env="test">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('DRM-Free Modals', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  describe('LocalGameEditor', () => {
    const mockReg: Registration = {
      id: 'reg-1',
      title: 'Hades',
      executable: 'Hades.exe',
      fileIdentity: 'hash-1',
      root: '/games/Hades',
      workingDirectory: '/games/Hades',
      arguments: ['-novid', '-high'],
      gameplay: ['Hades.exe'],
      launchers: [],
      revision: 1,
      removed: false,
      customCover: null,
      available: true,
    };

    const mockCandidate: Candidate = {
      fingerprint: 'fp-1',
      watch: '/games',
      root: '/games/Hades',
      title: 'Hades',
      targets: ['Hades.exe', 'Hades_DX11.exe'],
      selectedTarget: 'Hades.exe',
      evidence: [],
      ignored: false,
      incomplete: false,
    };

    it('renders editing fields and updates fields properly', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.registrations = [mockReg];
      uiStore.setState((s) => ({ ...s, localEditorEntryKey: 'manual:reg-1' }));

      renderWithProviders(<LocalGameEditor />, snap);

      expect(screen.getByRole('dialog')).toBeInTheDocument();
      expect(screen.getByTestId('local-editor-title')).toHaveValue('Hades');
      expect(screen.getByTestId('local-editor-executable')).toHaveValue('Hades.exe');
      expect(screen.getByTestId('local-editor-root')).toHaveValue('/games/Hades');
      expect(screen.getByTestId('local-editor-working-dir')).toHaveValue('/games/Hades');
      expect(screen.getByTestId('local-editor-arguments')).toHaveValue('-novid -high');
      expect(screen.getByTestId('local-editor-cover-placeholder')).toBeInTheDocument();
    });

    it('switches candidate executable when multiple targets exist', async () => {
      const snap = createDefaultSnapshot();
      snap.localGames.registrations = [mockReg];
      snap.localGames.candidates = [mockCandidate];

      renderWithProviders(<LocalGameEditor registration={mockReg} />, snap);

      const candidateSelect = screen.getByTestId('candidate-executable-select');
      expect(candidateSelect).toBeInTheDocument();
      expect(screen.getByTestId('local-editor-multiple-warning')).toBeInTheDocument();

      fireEvent.click(candidateSelect);
      fireEvent.click(await screen.findByRole('option', { name: 'Hades_DX11.exe' }));
      await waitFor(() =>
        expect(screen.getByTestId('local-editor-executable')).toHaveValue('Hades_DX11.exe'),
      );
    });

    it('hides the several-executables warning when only one target exists', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.registrations = [mockReg];
      snap.localGames.candidates = [{ ...mockCandidate, targets: ['Hades.exe'] }];

      renderWithProviders(<LocalGameEditor registration={mockReg} />, snap);

      expect(screen.queryByTestId('local-editor-multiple-warning')).not.toBeInTheDocument();
      expect(screen.queryByTestId('candidate-executable-select')).not.toBeInTheDocument();
    });

    it('keeps process detection collapsed at the inferred defaults', () => {
      renderWithProviders(<LocalGameEditor registration={mockReg} />);

      const advanced = screen.getByTestId('local-editor-advanced');
      expect(advanced).toHaveAttribute('aria-expanded', 'false');
      expect(screen.queryByTestId('local-editor-gameplay')).not.toBeInTheDocument();
      expect(screen.queryByTestId('local-editor-launchers')).not.toBeInTheDocument();

      fireEvent.click(advanced);
      expect(advanced).toHaveAttribute('aria-expanded', 'true');
      expect(screen.getByTestId('local-editor-gameplay')).toHaveValue('Hades.exe');
      expect(screen.getByTestId('local-editor-launchers')).toHaveValue('');
    });

    it('opens process detection when its values differ from the inferred defaults', () => {
      renderWithProviders(
        <LocalGameEditor
          registration={{
            ...mockReg,
            gameplay: ['Hades.exe', 'Hades_DX11.exe'],
            launchers: ['launcher.exe'],
          }}
        />,
      );

      expect(screen.getByTestId('local-editor-advanced')).toHaveAttribute(
        'aria-expanded',
        'true',
      );
      expect(screen.getByTestId('local-editor-gameplay')).toHaveValue(
        'Hades.exe\nHades_DX11.exe',
      );
      expect(screen.getByTestId('local-editor-launchers')).toHaveValue('launcher.exe');
    });

    it('calls pickLocalExecutable when Locate executable button is clicked', () => {
      const spy = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue(undefined);
      renderWithProviders(<LocalGameEditor registration={mockReg} />);

      const locateBtn = screen.getByRole('button', { name: /Locate executable/i });
      fireEvent.click(locateBtn);

      expect(spy).toHaveBeenCalledWith('reg-1');
    });

    it('picks a cover, previews it and saves the pending file', async () => {
      const pickSpy = vi
        .spyOn(commands, 'pickLocalCover')
        .mockResolvedValue('local-reg-1-0123456789abcdef.webp');
      const saveSpy = vi.spyOn(commands, 'editLocalGame').mockResolvedValue(undefined);
      renderWithProviders(<LocalGameEditor registration={mockReg} />);

      fireEvent.click(screen.getByTestId('local-editor-change-cover'));
      await waitFor(() =>
        expect(screen.getByTestId('local-editor-cover-img')).toBeInTheDocument(),
      );
      expect(pickSpy).toHaveBeenCalledWith('manual:reg-1');
      expect(screen.getByTestId('local-editor-cover-img').getAttribute('src')).toContain(
        'local-reg-1-0123456789abcdef.webp',
      );

      fireEvent.click(screen.getByTestId('local-editor-save'));
      expect(saveSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 'reg-1',
          customCover: 'local-reg-1-0123456789abcdef.webp',
        }),
      );
    });

    it('clears a committed cover and saves the GameNode cover', async () => {
      const custom = { ...mockReg, customCover: 'local-reg-1-fedcba9876543210.webp' };
      const discardSpy = vi
        .spyOn(commands, 'discardLocalCover')
        .mockResolvedValue(undefined);
      const saveSpy = vi.spyOn(commands, 'editLocalGame').mockResolvedValue(undefined);
      renderWithProviders(<LocalGameEditor registration={custom} />);

      expect(
        screen.getByTestId('local-editor-cover-img').getAttribute('src'),
      ).toContain('local-reg-1-fedcba9876543210.webp');

      fireEvent.click(screen.getByTestId('local-editor-use-gamenode-cover'));
      expect(discardSpy).toHaveBeenCalledWith('manual:reg-1');
      expect(
        screen.getByTestId('local-editor-cover-placeholder'),
      ).toBeInTheDocument();

      fireEvent.click(screen.getByTestId('local-editor-save'));
      expect(saveSpy).toHaveBeenCalledWith(
        expect.objectContaining({ id: 'reg-1', customCover: null }),
      );
    });

    it('discards a pending cover when the editor is cancelled', async () => {
      vi.spyOn(commands, 'pickLocalCover').mockResolvedValue(
        'local-reg-1-0123456789abcdef.webp',
      );
      const discardSpy = vi
        .spyOn(commands, 'discardLocalCover')
        .mockResolvedValue(undefined);
      const onClose = vi.fn();
      renderWithProviders(<LocalGameEditor registration={mockReg} onClose={onClose} />);

      fireEvent.click(screen.getByTestId('local-editor-change-cover'));
      await waitFor(() =>
        expect(screen.getByTestId('local-editor-cover-img')).toBeInTheDocument(),
      );
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

      expect(discardSpy).toHaveBeenCalledWith('manual:reg-1');
      expect(onClose).toHaveBeenCalled();
    });

    it('shows a localized error when the picked image is too large', async () => {
      vi.spyOn(commands, 'pickLocalCover').mockRejectedValue('coverTooLarge');
      renderWithProviders(<LocalGameEditor registration={mockReg} />);

      fireEvent.click(screen.getByTestId('local-editor-change-cover'));

      expect(await screen.findByTestId('local-editor-cover-error')).toHaveTextContent(
        'The image is larger than 20 MB.',
      );
      expect(screen.getByTestId('local-editor-cover-placeholder')).toBeInTheDocument();
    });

    it('saves updated registration and closes modal', () => {
      const spy = vi.spyOn(commands, 'editLocalGame').mockResolvedValue(undefined);
      const onClose = vi.fn();

      renderWithProviders(<LocalGameEditor registration={mockReg} onClose={onClose} />);

      fireEvent.change(screen.getByTestId('local-editor-title'), { target: { value: 'Hades Remastered' } });
      fireEvent.change(screen.getByTestId('local-editor-arguments'), { target: { value: '-novid --dx12' } });

      const saveBtn = screen.getByRole('button', { name: /^Save$/i });
      fireEvent.click(saveBtn);

      expect(spy).toHaveBeenCalledWith(
        expect.objectContaining({
          id: 'reg-1',
          title: 'Hades Remastered',
          arguments: ['-novid', '--dx12'],
        }),
      );
      expect(onClose).toHaveBeenCalled();
    });

    it('opens the removal confirmation from the footer', () => {
      const onClose = vi.fn();
      renderWithProviders(<LocalGameEditor registration={mockReg} onClose={onClose} />);

      fireEvent.click(screen.getByTestId('local-editor-remove'));

      expect(uiStore.state.localEditorEntryKey).toBeNull();
      expect(uiStore.state.removeModalEntryKey).toBe('manual:reg-1');
      expect(onClose).toHaveBeenCalled();
    });

    it('renders localized labels in pt-BR', () => {
      renderWithProviders(<LocalGameEditor registration={mockReg} />, undefined, 'pt-BR');

      expect(screen.getByText('Configurações do jogo')).toBeInTheDocument();
      expect(screen.getByText('Título')).toBeInTheDocument();
      expect(screen.getByText('Executável')).toBeInTheDocument();
      expect(screen.getByText('Pasta do jogo')).toBeInTheDocument();
      expect(screen.getByText('Diretório de trabalho')).toBeInTheDocument();
      expect(screen.getByText('Argumentos')).toBeInTheDocument();
      expect(screen.getByText('Trocar capa…')).toBeInTheDocument();
      expect(screen.getByText('Usar capa do GameNode')).toBeInTheDocument();
      expect(screen.getByText('Avançado · detecção de processo')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Salvar' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Cancelar' })).toBeInTheDocument();
      expect(
        screen.getByRole('button', { name: 'Remover da biblioteca…' }),
      ).toBeInTheDocument();
    });
  });

  describe('DiscoveryModal', () => {
    const candidate1: Candidate = {
      fingerprint: 'cand-1',
      watch: '/games',
      root: '/games/Celeste',
      title: 'Celeste',
      targets: ['Celeste.exe', 'Celeste.bin.x86_64'],
      selectedTarget: 'Celeste.exe',
      evidence: [],
      ignored: false,
      incomplete: false,
      suggestion: {
        gameId: 101,
        title: 'Celeste',
        year: 2018,
        coverUrl: null,
        confidence: 96,
        tone: 'success',
      },
      likelyNonGame: false,
    };

    const candidate2: Candidate = {
      fingerprint: 'cand-2',
      watch: '/games',
      root: '/games/DeadCells',
      title: 'Dead Cells',
      targets: ['deadcells.exe'],
      selectedTarget: 'deadcells.exe',
      evidence: [],
      ignored: false,
      incomplete: false,
      suggestion: null,
      likelyNonGame: false,
    };

    const nonGame: Candidate = {
      fingerprint: 'cand-3',
      watch: '/games',
      root: '/games/Instaladores',
      title: 'setup_redist',
      targets: ['vc_redist.x64.exe'],
      selectedTarget: 'vc_redist.x64.exe',
      evidence: [],
      ignored: false,
      incomplete: false,
      suggestion: null,
      likelyNonGame: true,
    };

    it('renders the table, suggestions and status banners', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.discoveryScanning = true;
      snap.localGames.discoveryError = 'Failed to access /network/share';
      snap.localGames.discoveryIncomplete = ['/games/Corrupted'];
      snap.localGames.candidates = [candidate1, candidate2, nonGame];
      snap.localGames.reviewSelected = ['cand-1', 'cand-2'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      expect(screen.getByTestId('discovery-table')).toBeInTheDocument();
      expect(screen.getByTestId('discovery-scanning-banner')).toBeInTheDocument();
      expect(screen.getByTestId('discovery-error-banner')).toHaveTextContent('Failed to access /network/share');
      expect(screen.getByTestId('discovery-incomplete-banner')).toHaveTextContent('/games/Corrupted');
      expect(screen.getByTestId('candidate-row-cand-1')).toBeInTheDocument();
      expect(screen.getByTestId('candidate-row-cand-3')).toBeInTheDocument();
      expect(screen.getAllByText('Celeste').length).toBeGreaterThan(0);
      expect(screen.getByText('Dead Cells')).toBeInTheDocument();
      expect(screen.getByText('No match')).toBeInTheDocument();
      expect(screen.getByText('Probably not a game')).toBeInTheDocument();
    });

    it('starts likely non-games unchecked and keeps a manual uncheck', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1, candidate2, nonGame];
      snap.localGames.reviewSelected = ['cand-1'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      expect(screen.getByTestId('candidate-checkbox-cand-1')).toBeChecked();
      expect(screen.getByTestId('candidate-checkbox-cand-2')).not.toBeChecked();
      expect(screen.getByTestId('candidate-checkbox-cand-3')).not.toBeChecked();
      expect(
        screen.getByRole('checkbox', { name: 'Select all' }),
      ).toBePartiallyChecked();
    });

    it('selects and clears every row from the header checkbox', () => {
      const selectSpy = vi.spyOn(commands, 'selectLocalCandidate').mockResolvedValue(undefined);
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1, candidate2, nonGame];
      snap.localGames.reviewSelected = ['cand-1'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      fireEvent.click(screen.getByRole('checkbox', { name: 'Select all' }));
      expect(selectSpy).toHaveBeenCalledWith('cand-2');
      expect(selectSpy).toHaveBeenCalledWith('cand-3');
      expect(selectSpy).not.toHaveBeenCalledWith('cand-1');
    });

    it('follows the selection in the Add label and sends the rediscovery choice', () => {
      const reviewSpy = vi.spyOn(commands, 'reviewSelectedLocalCandidates').mockResolvedValue(undefined);
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1, candidate2, nonGame];
      snap.localGames.reviewSelected = ['cand-1', 'cand-2'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      expect(screen.getByRole('button', { name: 'Add 2 games' })).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Add 2 games' }));
      expect(reviewSpy).toHaveBeenCalledWith(true, true);

      fireEvent.click(screen.getByTestId('allow-rediscovery-checkbox'));
      fireEvent.click(screen.getByRole('button', { name: 'Ignore selected' }));
      expect(reviewSpy).toHaveBeenCalledWith(false, false);
    });

    it('disables the footer controls without a selection', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1];
      snap.localGames.reviewSelected = [];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      expect(screen.getByTestId('allow-rediscovery-checkbox')).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Ignore selected' })).toBeDisabled();
      expect(screen.getByRole('button', { name: 'Add 0 games' })).toBeDisabled();
    });

    it('changes the selected executable from the compact select', async () => {
      const user = userEvent.setup();
      const targetSpy = vi.spyOn(commands, 'setLocalCandidateExecutable').mockResolvedValue(undefined);
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1];
      snap.localGames.reviewSelected = ['cand-1'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      await user.click(
        screen.getByRole('combobox', { name: 'Target executable for Celeste' }),
      );
      await user.click(
        await screen.findByRole('option', { name: 'Celeste.bin.x86_64' }),
      );
      expect(targetSpy).toHaveBeenCalledWith('cand-1', 'Celeste.bin.x86_64');
    });

    it('closes from the header button', () => {
      const dismissSpy = vi.spyOn(commands, 'dismissLocalDiscovered').mockResolvedValue(undefined);
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1];
      snap.localGames.reviewSelected = ['cand-1'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      fireEvent.click(screen.getByTestId('discovery-close-button'));
      expect(dismissSpy).toHaveBeenCalled();
    });

    it('renders localized labels in pt-BR', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1, candidate2, nonGame];
      snap.localGames.reviewSelected = ['cand-1', 'cand-2'];

      renderWithProviders(<DiscoveryModal open={true} />, snap, 'pt-BR');

      expect(screen.getByText('Jogos descobertos')).toBeInTheDocument();
      expect(screen.getByText('Ignorar selecionados')).toBeInTheDocument();
      expect(screen.getByText('Adicionar 2 jogos')).toBeInTheDocument();
      expect(
        screen.getByText('Permitir redescoberta em verificações futuras'),
      ).toBeInTheDocument();
      expect(screen.getByText('Sem correspondência')).toBeInTheDocument();
      expect(screen.getByText('Provavelmente não é jogo')).toBeInTheDocument();
    });
  });

  describe('ClaimConflictModal', () => {
    const pendingClaim: PendingClaim = {
      storeKey: 'steam:271590',
      storeEntry: {
        title: 'Grand Theft Auto V',
        provider: 'steam',
        coverFile: 'gta-cover.jpg',
        playtimeSeconds: 15 * 3600,
      },
      registration: {
        title: 'Grand Theft Auto V',
        root: '/games/GTA',
        coverFile: null,
      },
    };

    it('resolves with move tracking, use existing item, or cancel', () => {
      const resolveSpy = vi.spyOn(commands, 'resolveLocalClaim').mockResolvedValue(undefined);
      const cancelSpy = vi.spyOn(commands, 'cancelLocalClaim').mockResolvedValue(undefined);

      const { rerender } = renderWithProviders(<ClaimConflictModal claim={pendingClaim} />);

      expect(screen.getByRole('dialog')).toBeInTheDocument();
      expect(screen.getByTestId('claim-store-card')).toHaveTextContent('Grand Theft Auto V');
      expect(screen.getByTestId('claim-store-card')).toHaveTextContent('Steam · 15 h');
      expect(screen.getByTestId('claim-registration-card')).toHaveTextContent(
        'DRM-free · /games/GTA',
      );
      expect(screen.getByTestId('claim-store-cover').querySelector('img')).toHaveAttribute(
        'loading',
        'lazy',
      );

      // Move tracking
      fireEvent.click(screen.getByTestId('claim-resolve-transfer'));
      expect(resolveSpy).toHaveBeenCalledWith(true);

      // Use the existing item
      rerender(
        <MantineProvider theme={appTheme} defaultColorScheme="dark">
          <QueryClientProvider client={createAppQueryClient()}>
            <I18nProvider initialLocale="en">
              <ClaimConflictModal claim={pendingClaim} />
            </I18nProvider>
          </QueryClientProvider>
        </MantineProvider>,
      );
      fireEvent.click(screen.getByTestId('claim-resolve-separate'));
      expect(resolveSpy).toHaveBeenCalledWith(false);

      // Cancel
      fireEvent.click(screen.getByTestId('claim-cancel'));
      expect(cancelSpy).toHaveBeenCalled();
    });

    it('uses a store icon that follows the provider instead of assuming Steam', () => {
      const { rerender } = renderWithProviders(<ClaimConflictModal claim={pendingClaim} />);
      expect(
        screen.getByTestId('claim-store-provider-icon').querySelector('svg')?.getAttribute('class'),
      ).toContain('brand-steam');

      const epicClaim: PendingClaim = {
        ...pendingClaim,
        storeEntry: { ...pendingClaim.storeEntry, provider: 'epic' },
      };
      rerender(
        <MantineProvider theme={appTheme} defaultColorScheme="dark">
          <QueryClientProvider client={createAppQueryClient()}>
            <I18nProvider initialLocale="en">
              <ClaimConflictModal claim={epicClaim} />
            </I18nProvider>
          </QueryClientProvider>
        </MantineProvider>,
      );
      const epicIcon = screen
        .getByTestId('claim-store-provider-icon')
        .querySelector('svg')
        ?.getAttribute('class');
      expect(epicIcon).not.toContain('brand-steam');
      expect(epicIcon).toContain('gamepad');
    });

    it('renders localized modal in pt-BR', () => {
      renderWithProviders(<ClaimConflictModal claim={pendingClaim} />, undefined, 'pt-BR');

      expect(screen.getByText('Este jogo já é monitorado por uma loja')).toBeInTheDocument();
      expect(screen.getByTestId('claim-resolve-separate')).toHaveTextContent('Usar item existente');
      expect(screen.getByTestId('claim-resolve-transfer')).toHaveTextContent('Transferir monitoramento');
      expect(screen.getByTestId('claim-cancel')).toHaveTextContent('Cancelar');
    });
  });

  describe('RemoveLocalGameModal', () => {
    const mockReg: Registration = {
      id: 'hollow-knight',
      title: 'Hollow Knight',
      executable: 'hollow_knight.exe',
      fileIdentity: 'hk-hash',
      root: '/games/HK',
      workingDirectory: '/games/HK',
      arguments: [],
      gameplay: [],
      launchers: [],
      revision: 1,
      removed: false,
      available: true,
    };

    it('preselects the safer choice, focuses Cancel and sends allowRediscovery=false', async () => {
      const removeEntrySpy = vi.spyOn(commands, 'removeEntry').mockResolvedValue(undefined);
      const removeLocalSpy = vi.spyOn(commands, 'removeLocalGame').mockResolvedValue(undefined);

      renderWithProviders(
        <RemoveLocalGameModal entryKey="manual:hollow-knight" registration={mockReg} open={true} />,
      );

      expect(
        screen.getByRole('heading', { name: 'Remove Hollow Knight from the library?' }),
      ).toBeInTheDocument();
      expect(
        screen.getByRole('radio', { name: "Remove and don't import again" }),
      ).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Just remove' })).not.toBeChecked();
      await waitFor(() =>
        expect(screen.getByTestId('remove-cancel-button')).toHaveFocus(),
      );

      fireEvent.click(screen.getByTestId('confirm-remove-button'));

      expect(removeEntrySpy).toHaveBeenCalledWith('manual:hollow-knight', false);
      expect(removeLocalSpy).toHaveBeenCalledWith('hollow-knight', false);
    });

    it('sends allowRediscovery=true when Just remove is chosen', () => {
      const removeEntrySpy = vi.spyOn(commands, 'removeEntry').mockResolvedValue(undefined);
      const removeLocalSpy = vi.spyOn(commands, 'removeLocalGame').mockResolvedValue(undefined);

      renderWithProviders(
        <RemoveLocalGameModal entryKey="manual:hollow-knight" registration={mockReg} open={true} />,
      );

      fireEvent.click(screen.getByTestId('remove-choice-rediscovery'));
      expect(screen.getByRole('radio', { name: 'Just remove' })).toBeChecked();

      fireEvent.click(screen.getByTestId('confirm-remove-button'));

      expect(removeEntrySpy).toHaveBeenCalledWith('manual:hollow-knight', true);
      expect(removeLocalSpy).toHaveBeenCalledWith('hollow-knight', true);
    });

    it('keeps the open-folder ghost action for a local registration', () => {
      const openFolderSpy = vi
        .spyOn(commands, 'openInstallFolder')
        .mockResolvedValue(undefined);

      renderWithProviders(
        <RemoveLocalGameModal entryKey="manual:hollow-knight" registration={mockReg} open={true} />,
      );

      fireEvent.click(screen.getByTestId('remove-open-folder'));
      expect(openFolderSpy).toHaveBeenCalledWith('/games/HK');
    });

    it('uses store wording and removes a store entry without touching local registrations', () => {
      const removeEntrySpy = vi.spyOn(commands, 'removeEntry').mockResolvedValue(undefined);
      const removeLocalSpy = vi
        .spyOn(commands, 'removeLocalGame')
        .mockResolvedValue(undefined);

      const snapshot = createDefaultSnapshot();
      snapshot.library = {
        ...createDefaultLibraryView(),
        generation: 1,
        layout: {
          view: 'grid',
          sort: { key: 'title', direction: 'ascending' },
          scope: 'all',
          group: 'none',
          collapsedGroups: {},
          filters: createDefaultFilters(),
          sidebarWidth: 200,
          detailWidth: 320,
          detailOpen: false,
          sidebarCollapsed: false,
        },
        search: '',
        access: 'verified',
        total: 1,
        installedTotal: 1,
        providers: [{ id: 'steam', name: 'Steam', count: 1, installedCount: 1, ownedCount: 1, sharedCount: 0, sharedLabel: 'Steam Family' }],
        rows: [
          {
            key: 'steam:620',
            title: 'Portal 2',
            detectedName: 'Portal 2',
            matchedName: 'Portal 2',
            provider: 'steam',
            providerName: 'Steam',
            installed: true,
            owned: true,
            shared: false,
            running: false,
            status: { matched: { gameId: 7, source: 'automatic' } },
            coverFile: null,
            playtime: { seconds: 0, source: 'store', lastPlayedAt: null },
            releaseYear: 2011,
            addedAt: 0,
            isFavorite: false,
            hidden: false,
            gamenode: null,
          },
        ],
        scanning: false,
        failedProviders: [],
        selected: ['steam:620'],
        focused: 'steam:620',
        actions: {
          selected: 1,
          play: 'available',
          install: 'available',
          matchGame: 'available',
          addToGamenode: 'available',
          changeMatch: false,
        },
      };

      renderWithProviders(
        <RemoveLocalGameModal entryKey="steam:620" open={true} />,
        snapshot,
      );

      expect(
        screen.getByRole('heading', { name: 'Remove Portal 2 from the library?' }),
      ).toBeInTheDocument();
      expect(
        screen.getByText('It can come back at the next scan.'),
      ).toBeInTheDocument();
      expect(
        screen.getByText('The game stays hidden. Undo it in Settings › Stores.'),
      ).toBeInTheDocument();
      expect(screen.queryByTestId('remove-open-folder')).toBeNull();

      fireEvent.click(screen.getByTestId('remove-choice-rediscovery'));
      fireEvent.click(screen.getByTestId('confirm-remove-button'));

      expect(removeEntrySpy).toHaveBeenCalledWith('steam:620', true);
      expect(removeLocalSpy).not.toHaveBeenCalled();
    });

    it('renders localized modal in pt-BR', () => {
      renderWithProviders(
        <RemoveLocalGameModal entryKey="manual:hollow-knight" registration={mockReg} open={true} />,
        undefined,
        'pt-BR',
      );

      expect(
        screen.getByRole('heading', { name: 'Remover Hollow Knight da biblioteca?' }),
      ).toBeInTheDocument();
      expect(screen.getByTestId('confirm-remove-button')).toHaveTextContent('Remover');
      expect(screen.getByTestId('remove-cancel-button')).toHaveTextContent('Cancelar');
      expect(screen.getByRole('radio', { name: 'Remover e não importar mais' })).toBeChecked();
      expect(screen.getByRole('radio', { name: 'Apenas remover' })).not.toBeChecked();
    });
  });

  describe('SaveProtectionModal', () => {
    const profile: SaveProfile = {
      id: 'sp-1',
      installation: 'game-123',
      revision: 1,
      title: 'Cyberpunk 2077',
      manifestHash: 'hash-cp',
      definition: 'ludusavi-def',
      files: ['/home/cassi/.local/share/Cyberpunk 2077/saves'],
      registry: ['HKEY_CURRENT_USER\\Software\\CD Projekt Red\\Cyberpunk 2077'],
      support: 'supported',
      automatic: true,
    };

    it('displays save profile summary, protected files, and registry keys', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      uiStore.setState((s) => ({ ...s, saveProtectionEntryKey: 'manual:game-123' }));

      renderWithProviders(<SaveProtectionModal />, snap);

      expect(screen.getByText(/Cyberpunk 2077 · DRM-free/)).toBeInTheDocument();
      expect(screen.getByTestId('save-support-badge')).toHaveTextContent('Supported');
      fireEvent.click(screen.getByTestId('protected-files-toggle'));
      expect(screen.getByText('/home/cassi/.local/share/Cyberpunk 2077/saves')).toBeInTheDocument();
      expect(screen.getByText('HKEY_CURRENT_USER\\Software\\CD Projekt Red\\Cyberpunk 2077')).toBeInTheDocument();
    });

    it('displays overlapping-saves warning banner when another installation shares save title or files', () => {
      const otherProfile: SaveProfile = {
        id: 'sp-2',
        installation: 'game-456',
        revision: 1,
        title: 'Cyberpunk 2077',
        manifestHash: 'hash-cp2',
        definition: 'ludusavi-def',
        files: ['/home/cassi/.local/share/Cyberpunk 2077/saves'],
        registry: [],
        support: 'supported',
        automatic: true,
      };

      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile, otherProfile];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      expect(screen.getByTestId('overlapping-saves-banner')).toBeInTheDocument();
      expect(screen.getByTestId('overlapping-saves-banner')).toHaveTextContent(
        'Another installation shares these saves',
      );
    });

    it('toggles automatic backup and configures custom saves', () => {
      const toggleSpy = vi.spyOn(commands, 'setLocalProfileAutomatic').mockResolvedValue(undefined);
      const configureSpy = vi.spyOn(commands, 'configureLocalSaves').mockResolvedValue(undefined);
      const pickSpy = vi.spyOn(commands, 'pickLocalSaves').mockResolvedValue(undefined);
      const chooseProfileSpy = vi.spyOn(commands, 'chooseLocalSaveProfile').mockResolvedValue(undefined);

      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      // Automatic toggle
      const autoToggle = screen.getByTestId('automatic-backup-toggle');
      expect(autoToggle).toBeChecked();
      fireEvent.click(autoToggle);
      expect(toggleSpy).toHaveBeenCalledWith('game-123', false);

      fireEvent.click(screen.getByTestId('protected-files-toggle'));

      // Browse folder & file
      fireEvent.click(screen.getByRole('button', { name: /Change save folder/i }));
      expect(pickSpy).toHaveBeenCalledWith('game-123', true);

      fireEvent.click(screen.getByRole('button', { name: /Change save file/i }));
      expect(pickSpy).toHaveBeenCalledWith('game-123', false);

      // Configure custom saves
      const customInput = screen.getByPlaceholderText(/Custom save file or folder/i);
      fireEvent.change(customInput, { target: { value: '/custom/save/dir' } });
      fireEvent.click(screen.getByRole('button', { name: /Use this save location/i }));
      expect(configureSpy).toHaveBeenCalledWith('game-123', '/custom/save/dir');

      // Change profile title
      const profileInput = screen.getByPlaceholderText(/Exact save profile title/i);
      fireEvent.change(profileInput, { target: { value: 'Cyberpunk 2077 (GOG)' } });
      fireEvent.click(screen.getByRole('button', { name: /Change save profile/i }));
      expect(chooseProfileSpy).toHaveBeenCalledWith('game-123', 'Cyberpunk 2077 (GOG)');
    });

    it('triggers Backup now button and shows live job state badge', () => {
      const backupSpy = vi.spyOn(commands, 'backupLocalNow').mockResolvedValue(undefined);

      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      snap.localGames.jobs = [
        {
          id: 'job-1',
          installation: 'game-123',
          profile: 'sp-1',
          revision: 1,
          destination: '/backups/cp2077',
          state: 'successful',
          attempts: 1,
          readyAt: Date.now(),
        },
      ];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      expect(screen.getByTestId('backup-job-state-badge')).toHaveTextContent(
        'Backup completed',
      );

      const backupBtn = screen.getByTestId('backup-local-now-button');
      fireEvent.click(backupBtn);
      expect(backupSpy).toHaveBeenCalledWith('game-123');
    });

    it('renders backup history and triggers restore preview', () => {
      const restoreSpy = vi.spyOn(commands, 'previewLocalRestore').mockResolvedValue(undefined);

      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      snap.localGames.versions = [
        {
          installation: 'game-123',
          profile: 'sp-1',
          destination: '/backups/cp2077',
          id: 'v-1',
          label: '2026-11-06 12:00:00',
          bytes: 1048576 * 15, // 15 MB
        },
      ];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      expect(screen.getByText('2026-11-06 12:00:00')).toBeInTheDocument();
      expect(screen.getByText(/15.0 MB/)).toBeInTheDocument();

      const restoreBtn = screen.getByRole('button', { name: /^Restore$/i });
      fireEvent.click(restoreBtn);

      const expectedVersionKey = `4:sp-1:3:v-1:/backups/cp2077`;
      expect(restoreSpy).toHaveBeenCalledWith(expectedVersionKey);
    });

    it('renders two-step restore preview and confirms / cancels restore', () => {
      const confirmSpy = vi.spyOn(commands, 'confirmLocalRestore').mockResolvedValue(undefined);
      const cancelSpy = vi.spyOn(commands, 'cancelLocalRestore').mockResolvedValue(undefined);

      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      snap.localGames.restore = {
        request: {
          profile,
          installation: {
            id: 'game-123',
            title: 'Cyberpunk 2077',
            executable: 'bin/x64/Cyberpunk2077.exe',
            fileIdentity: 'hash',
            root: '/games/cp2077',
            workingDirectory: '/games/cp2077',
            arguments: [],
            gameplay: [],
            launchers: [],
            revision: 1,
            removed: false,
            available: true,
          },
          destination: '/backups',
          operation: 'preview',
        },
        targets: ['/home/cassi/.local/share/Cyberpunk 2077/saves/manualsave_0.sav'],
      };

      const { rerender } = renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      expect(screen.getByTestId('restore-confirmation-view')).toBeInTheDocument();
      expect(
        screen.getByText('/home/cassi/.local/share/Cyberpunk 2077/saves/manualsave_0.sav'),
      ).toBeInTheDocument();

      // Confirm
      fireEvent.click(screen.getByRole('button', { name: /Confirm restore/i }));
      expect(confirmSpy).toHaveBeenCalled();

      // Cancel
      rerender(
        <MantineProvider theme={appTheme} defaultColorScheme="dark">
          <QueryClientProvider client={createAppQueryClient()}>
            <I18nProvider initialLocale="en">
              <SaveProtectionModal installationId="game-123" />
            </I18nProvider>
          </QueryClientProvider>
        </MantineProvider>,
      );
      // Escape or cancel button
      fireEvent.click(screen.getByRole('button', { name: /Cancel/i }));
      expect(cancelSpy).toHaveBeenCalled();
    });

    it('renders localized SaveProtectionModal in pt-BR', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap, 'pt-BR');

      expect(screen.getByText('Proteção local dos saves')).toBeInTheDocument();
      expect(screen.getByText('Proteção ativa')).toBeInTheDocument();
      expect(screen.getByText('Espaço usado')).toBeInTheDocument();
      expect(screen.getByText('Fazer cópia agora')).toBeInTheDocument();
      expect(screen.getByText('Manter')).toBeInTheDocument();
    });

    it('renders every history row, the disclosure and the override without clipping the body', () => {
      const now = Math.floor(Date.now() / 1000);
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      snap.localGames.versions = Array.from({ length: 6 }, (_, index) => ({
        installation: 'game-123',
        profile: 'sp-1',
        destination: '/backups/cp2077',
        id: `row-${index}`,
        label: `2026-11-${15 - index} 14:20:00`,
        bytes: 7_100_000 - index * 1_000,
        createdAt: now - index * 3_600,
        reason: index === 3 ? ('manual' as const) : ('afterSession' as const),
        playSeconds: 42 * 60,
      }));

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      const history = screen.getByTestId('backup-history');
      expect(history).toHaveClass('shrink-0');
      expect(within(history).getAllByRole('button', { name: /^Restore$/i })).toHaveLength(6);
      for (let index = 0; index < 6; index += 1) {
        expect(screen.getByTestId(`backup-version-row-${index}`)).toBeInTheDocument();
      }

      // The disclosure and the profile override stay reachable after the long history.
      fireEvent.click(screen.getByTestId('protected-files-toggle'));
      expect(screen.getByRole('button', { name: /Change save folder/i })).toBeInTheDocument();
      expect(screen.getByText('/home/cassi/.local/share/Cyberpunk 2077/saves')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /Use this save location/i })).toBeInTheDocument();
      expect(screen.getByTestId('backup-local-now-button')).toBeInTheDocument();
      expect(
        screen.getAllByRole('button', { name: /^Close$/i }).length,
      ).toBeGreaterThan(0);
    });

    it('labels every backup origin and leaves unknown origins without a reason', () => {
      const now = Math.floor(Date.now() / 1000);
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      snap.localGames.versions = [
        {
          installation: 'game-123',
          profile: 'sp-1',
          destination: '/backups/cp2077',
          id: 'after',
          label: '2026-11-15 14:20:00',
          bytes: 7_100_000,
          createdAt: now - 3_600,
          reason: 'afterSession',
          playSeconds: 42 * 60,
        },
        {
          installation: 'game-123',
          profile: 'sp-1',
          destination: '/backups/cp2077',
          id: 'manual',
          label: '2026-11-14 18:30:00',
          bytes: 6_800_000,
          createdAt: now - 86_400,
          reason: 'manual',
          playSeconds: null,
        },
        {
          installation: 'game-123',
          profile: 'sp-1_recovery_1',
          destination: '/backups/cp2077',
          id: 'recovery',
          label: '2026-11-10 21:44:00',
          bytes: 6_800_000,
          createdAt: now - 5 * 86_400,
          reason: 'recovery',
          playSeconds: null,
        },
        {
          installation: 'game-123',
          profile: 'sp-1',
          destination: '/backups/cp2077',
          id: 'unknown',
          label: '3 nov., 09:10',
          bytes: 6_700_000,
        },
      ];
      snap.localGames.historyBytes = 27_400_000;
      snap.localGames.historyCount = 4;

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      expect(screen.getByText('After a play session · 42 min')).toBeInTheDocument();
      expect(screen.getByText('Manual')).toBeInTheDocument();
      expect(screen.getByText('Recovery backup')).toBeInTheDocument();
      const unknown = screen.getByTestId('backup-version-unknown');
      expect(unknown).toHaveAttribute('data-reason', 'unknown');
      expect(unknown).not.toHaveTextContent('After a play session');
      expect(unknown).not.toHaveTextContent('Manual');
      expect(unknown).toHaveTextContent('3 nov., 09:10');
      expect(screen.getByLabelText('Space used')).toHaveValue(
        '26.1 MB in 4 copies',
      );
    });

    it('changes retention through the host command without pruning locally', () => {
      const retentionSpy = vi
        .spyOn(commands, 'setLocalBackupRetention')
        .mockResolvedValue(undefined);
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [{ ...profile, retention: 20 }];
      snap.localGames.versions = [
        {
          installation: 'game-123',
          profile: 'sp-1',
          destination: '/backups/cp2077',
          id: 'v-1',
          label: '2026-11-06 12:00:00',
          bytes: 1_048_576,
        },
      ];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap);

      fireEvent.click(screen.getByRole('combobox', { name: 'Keep' }));
      fireEvent.click(screen.getByText('50 most recent copies'));
      expect(retentionSpy).toHaveBeenCalledWith('game-123', 50);
      expect(screen.getByTestId('backup-version-v-1')).toBeInTheDocument();
    });

    it('renders backup origin copy in pt-BR', () => {
      const now = Math.floor(Date.now() / 1000);
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      snap.localGames.versions = [
        {
          installation: 'game-123',
          profile: 'sp-1',
          destination: '/backups/cp2077',
          id: 'after',
          label: '2026-11-15 14:20:00',
          bytes: 7_100_000,
          createdAt: now - 3_600,
          reason: 'afterSession',
          playSeconds: 42 * 60,
        },
        {
          installation: 'game-123',
          profile: 'sp-1_recovery_1',
          destination: '/backups/cp2077',
          id: 'recovery',
          label: '2026-11-10 21:44:00',
          bytes: 6_800_000,
          createdAt: now - 5 * 86_400,
          reason: 'recovery',
          playSeconds: null,
        },
      ];

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snap, 'pt-BR');

      expect(screen.getByText('Após sessão de jogo · 42 min')).toBeInTheDocument();
      expect(screen.getByText('Backup de recuperação')).toBeInTheDocument();
      expect(screen.getByText('Histórico de backups')).toBeInTheDocument();
    });

    it('renders localized two-step restore preview in pt-BR', () => {
      const snapRestore = createDefaultSnapshot();
      snapRestore.localGames.profiles = [profile];
      snapRestore.localGames.restore = {
        request: {
          profile,
          installation: {
            id: 'game-123',
            title: 'Cyberpunk 2077',
            executable: 'bin/x64/Cyberpunk2077.exe',
            fileIdentity: 'hash',
            root: '/games/cp2077',
            workingDirectory: '/games/cp2077',
            arguments: [],
            gameplay: [],
            launchers: [],
            revision: 1,
            removed: false,
            available: true,
          },
          destination: '/backups',
          operation: 'preview',
        },
        targets: ['/home/cassi/.local/share/Cyberpunk 2077/saves/manualsave_0.sav'],
      };

      renderWithProviders(<SaveProtectionModal installationId="game-123" />, snapRestore, 'pt-BR');

      expect(screen.getByText('Prévia da restauração')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Confirmar restauração' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Cancelar' })).toBeInTheDocument();
    });
  });
});
