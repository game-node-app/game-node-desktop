import { describe, expect, it, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Candidate,
  type PendingClaim,
  type Registration,
  type SaveProfile,
  type Snapshot,
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
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const snap = initialSnapshot ?? createDefaultSnapshot();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snap);

  return render(
    <QueryClientProvider client={queryClient}>
      <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
    </QueryClientProvider>,
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
    });

    it('switches candidate executable when multiple targets exist', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.registrations = [mockReg];
      snap.localGames.candidates = [mockCandidate];

      renderWithProviders(<LocalGameEditor registration={mockReg} />, snap);

      const candidateSelect = screen.getByTestId('candidate-executable-select');
      expect(candidateSelect).toBeInTheDocument();

      fireEvent.change(candidateSelect, { target: { value: 'Hades_DX11.exe' } });
      expect(screen.getByTestId('local-editor-executable')).toHaveValue('Hades_DX11.exe');
    });

    it('calls pickLocalExecutable when Locate executable button is clicked', () => {
      const spy = vi.spyOn(commands, 'pickLocalExecutable').mockResolvedValue(undefined);
      renderWithProviders(<LocalGameEditor registration={mockReg} />);

      const locateBtn = screen.getByRole('button', { name: /Locate executable/i });
      fireEvent.click(locateBtn);

      expect(spy).toHaveBeenCalledWith('reg-1');
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

    it('renders localized labels in pt-BR', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.registrations = [mockReg];

      renderWithProviders(<LocalGameEditor registration={mockReg} />, snap, 'pt-BR');

      expect(screen.getByText('Configurações do jogo')).toBeInTheDocument();
      expect(screen.getByText('Título')).toBeInTheDocument();
      expect(screen.getByText('Executável')).toBeInTheDocument();
      expect(screen.getByText('Pasta do jogo')).toBeInTheDocument();
      expect(screen.getByText('Diretório de trabalho')).toBeInTheDocument();
      expect(screen.getByText('Argumentos')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Salvar' })).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Cancelar' })).toBeInTheDocument();
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
    };

    it('renders watched-folder candidates and status banners', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.discoveryScanning = true;
      snap.localGames.discoveryError = 'Failed to access /network/share';
      snap.localGames.discoveryIncomplete = ['/games/Corrupted'];
      snap.localGames.candidates = [candidate1, candidate2];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      expect(screen.getByTestId('discovery-scanning-banner')).toBeInTheDocument();
      expect(screen.getByTestId('discovery-error-banner')).toHaveTextContent('Failed to access /network/share');
      expect(screen.getByTestId('discovery-incomplete-banner')).toHaveTextContent('/games/Corrupted');
      expect(screen.getByText('Celeste')).toBeInTheDocument();
      expect(screen.getByText('Dead Cells')).toBeInTheDocument();
    });

    it('toggles candidate selection and changes candidate target executable', () => {
      const selectSpy = vi.spyOn(commands, 'selectLocalCandidate').mockResolvedValue(undefined);
      const targetSpy = vi.spyOn(commands, 'setLocalCandidateExecutable').mockResolvedValue(undefined);

      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1];
      snap.localGames.reviewSelected = [];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      const checkbox = screen.getByTestId('candidate-checkbox-cand-1');
      fireEvent.click(checkbox);
      expect(selectSpy).toHaveBeenCalledWith('cand-1');

      const select = screen.getByTestId('candidate-target-select-cand-1');
      fireEvent.change(select, { target: { value: 'Celeste.bin.x86_64' } });
      expect(targetSpy).toHaveBeenCalledWith('cand-1', 'Celeste.bin.x86_64');
    });

    it('triggers Add selected, Ignore selected, Rescan, and Close actions', () => {
      const addSpy = vi.spyOn(commands, 'reviewSelectedLocalCandidates').mockResolvedValue(undefined);
      const rescanSpy = vi.spyOn(commands, 'rescanLocalFolders').mockResolvedValue(undefined);
      const dismissSpy = vi.spyOn(commands, 'dismissLocalDiscovered').mockResolvedValue(undefined);

      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1];
      snap.localGames.reviewSelected = ['cand-1'];

      renderWithProviders(<DiscoveryModal open={true} />, snap);

      fireEvent.click(screen.getByRole('button', { name: /Add selected/i }));
      expect(addSpy).toHaveBeenCalledWith(true);

      fireEvent.click(screen.getByRole('button', { name: /Ignore selected/i }));
      expect(addSpy).toHaveBeenCalledWith(false);

      fireEvent.click(screen.getByRole('button', { name: /Rescan/i }));
      expect(rescanSpy).toHaveBeenCalled();

      fireEvent.click(screen.getByTestId('discovery-close-button'));
      expect(dismissSpy).toHaveBeenCalled();
    });

    it('renders localized labels in pt-BR', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.candidates = [candidate1];
      snap.localGames.reviewSelected = ['cand-1'];

      renderWithProviders(<DiscoveryModal open={true} />, snap, 'pt-BR');

      expect(screen.getByText('Pastas monitoradas')).toBeInTheDocument();
      expect(screen.getByText('Ignorar selecionados')).toBeInTheDocument();
      expect(screen.getByText('Adicionar selecionados')).toBeInTheDocument();
      expect(screen.getByText('Verificar pastas monitoradas')).toBeInTheDocument();
      expect(screen.getByText('Fechar')).toBeInTheDocument();
    });
  });

  describe('ClaimConflictModal', () => {
    const pendingClaim: PendingClaim = {
      registration: {
        id: 'local-gta',
        title: 'Grand Theft Auto V',
        executable: 'GTA5.exe',
        fileIdentity: 'hash-gta',
        root: '/games/GTA',
        workingDirectory: '/games/GTA',
        arguments: [],
        gameplay: [],
        launchers: [],
        revision: 1,
        removed: false,
        available: true,
      },
      storeKey: 'steam:271590',
      storeTitle: 'Grand Theft Auto V (Steam)',
    };

    it('renders manual and store game titles and resolves transfer vs keep separate', () => {
      const resolveSpy = vi.spyOn(commands, 'resolveLocalClaim').mockResolvedValue(undefined);
      const cancelSpy = vi.spyOn(commands, 'cancelLocalClaim').mockResolvedValue(undefined);

      const { rerender } = renderWithProviders(<ClaimConflictModal claim={pendingClaim} />);

      expect(screen.getByRole('dialog')).toBeInTheDocument();
      expect(screen.getAllByText('Grand Theft Auto V').length).toBeGreaterThan(0);
      expect(screen.getAllByText('Grand Theft Auto V (Steam)').length).toBeGreaterThan(0);

      // Transfer
      fireEvent.click(screen.getByTestId('claim-resolve-transfer'));
      expect(resolveSpy).toHaveBeenCalledWith(true);

      // Keep separate
      rerender(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <I18nProvider initialLocale="en">
            <ClaimConflictModal claim={pendingClaim} />
          </I18nProvider>
        </QueryClientProvider>,
      );
      fireEvent.click(screen.getByTestId('claim-resolve-separate'));
      expect(resolveSpy).toHaveBeenCalledWith(false);

      // Cancel
      rerender(
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <I18nProvider initialLocale="en">
            <ClaimConflictModal claim={pendingClaim} />
          </I18nProvider>
        </QueryClientProvider>,
      );
      fireEvent.click(screen.getByRole('button', { name: /Cancel/i }));
      expect(cancelSpy).toHaveBeenCalled();
    });

    it('renders localized modal in pt-BR', () => {
      renderWithProviders(<ClaimConflictModal claim={pendingClaim} />, undefined, 'pt-BR');

      expect(screen.getByText('Este jogo já é monitorado por uma loja')).toBeInTheDocument();
      expect(screen.getByTestId('claim-resolve-separate')).toHaveTextContent('Usar item existente');
      expect(screen.getByTestId('claim-resolve-transfer')).toHaveTextContent('Transferir monitoramento');
      expect(screen.getByRole('button', { name: 'Cancelar' })).toBeInTheDocument();
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

    it('toggles allow-rediscovery checkbox and confirms removal', () => {
      const removeEntrySpy = vi.spyOn(commands, 'removeEntry').mockResolvedValue(undefined);
      const removeLocalSpy = vi.spyOn(commands, 'removeLocalGame').mockResolvedValue(undefined);

      renderWithProviders(
        <RemoveLocalGameModal entryKey="manual:hollow-knight" registration={mockReg} open={true} />,
      );

      expect(screen.getAllByText('Hollow Knight').length).toBeGreaterThan(0);
      const checkbox = screen.getByTestId('allow-rediscovery-checkbox');
      expect(checkbox).not.toBeChecked();

      fireEvent.click(checkbox);
      expect(checkbox).toBeChecked();

      const removeBtn = screen.getByTestId('confirm-remove-button');
      fireEvent.click(removeBtn);

      expect(removeEntrySpy).toHaveBeenCalledWith('manual:hollow-knight', true);
      expect(removeLocalSpy).toHaveBeenCalledWith('hollow-knight', true);
    });

    it('renders localized modal in pt-BR', () => {
      renderWithProviders(
        <RemoveLocalGameModal entryKey="manual:hollow-knight" registration={mockReg} open={true} />,
        undefined,
        'pt-BR',
      );

      expect(screen.getByRole('heading', { name: 'Remover da biblioteca' })).toBeInTheDocument();
      expect(screen.getByTestId('confirm-remove-button')).toHaveTextContent('Remover da biblioteca');
      expect(screen.getByRole('button', { name: 'Cancelar' })).toBeInTheDocument();
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

    it('displays save profile summary, files, and registry keys', () => {
      const snap = createDefaultSnapshot();
      snap.localGames.profiles = [profile];
      uiStore.setState((s) => ({ ...s, saveProtectionEntryKey: 'manual:game-123' }));

      renderWithProviders(<SaveProtectionModal />, snap);

      expect(screen.getAllByText('Cyberpunk 2077').length).toBeGreaterThan(0);
      expect(screen.getByTestId('save-support-badge')).toHaveTextContent('supported');
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

      // Browse folder & file
      fireEvent.click(screen.getByRole('button', { name: /Browse folder/i }));
      expect(pickSpy).toHaveBeenCalledWith('game-123', true);

      fireEvent.click(screen.getByRole('button', { name: /Browse file/i }));
      expect(pickSpy).toHaveBeenCalledWith('game-123', false);

      // Configure custom saves
      const customInput = screen.getByPlaceholderText(/Custom save file or folder/i);
      fireEvent.change(customInput, { target: { value: '/custom/save/dir' } });
      fireEvent.click(screen.getByRole('button', { name: /Save path/i }));
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

      expect(screen.getByTestId('backup-job-state-badge')).toHaveTextContent('successful');

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
        <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
          <I18nProvider initialLocale="en">
            <SaveProtectionModal installationId="game-123" />
          </I18nProvider>
        </QueryClientProvider>,
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
      expect(screen.getByText('Fazer backup deste jogo automaticamente')).toBeInTheDocument();
      expect(screen.getByText('Fazer backup agora')).toBeInTheDocument();
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
