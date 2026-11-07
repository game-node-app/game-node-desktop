import { describe, expect, it, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
  type ToolModalView,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { GameToolsModal } from './index';

function renderWithProviders(
  ui: React.ReactElement,
  initialSnapshot?: Snapshot,
  locale: 'en' | 'pt-BR' = 'en',
) {
  const queryClient = createAppQueryClient();
  const snap = initialSnapshot ?? createDefaultSnapshot();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snap);

  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('GameToolsModal', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  const baseModal: ToolModalView = {
    entryKey: 'steam:1091500',
    title: 'Cyberpunk 2077',
    installDir: '/games/Cyberpunk2077',
    activeToolId: 'optiscaler',
    selectedChannel: 'stable',
    availableVersions: [
      {
        toolId: 'optiscaler',
        channel: 'stable',
        version: '0.7.2',
        tagName: 'v0.7.2',
        name: 'OptiScaler 0.7.2',
        publishedAt: '2026-05-01',
        downloadUrl: 'https://github.com/optiscaler/OptiScaler/releases/download/v0.7.2/OptiScaler.7z',
        assetName: 'OptiScaler.7z',
        sizeBytes: 15000000,
        cachedPath: '/cache/OptiScaler.7z',
      },
      {
        toolId: 'optiscaler',
        channel: 'stable',
        version: '0.7.1',
        tagName: 'v0.7.1',
        name: 'OptiScaler 0.7.1',
        publishedAt: '2026-04-10',
        downloadUrl: 'https://github.com/optiscaler/OptiScaler/releases/download/v0.7.1/OptiScaler.7z',
        assetName: 'OptiScaler.7z',
        sizeBytes: 14500000,
      },
    ],
    selectedVersion: '0.7.2',
    selectedProxy: 'dxgi',
    anticheatConfirmed: false,
    inspection: {
      entryKey: 'steam:1091500',
      installDir: '/games/Cyberpunk2077',
      candidates: [
        {
          relativeExe: 'bin/x64/Cyberpunk2077.exe',
          label: 'Cyberpunk 2077 Main',
          architecture: 'x64',
          preferred: true,
        },
        {
          relativeExe: 'bin/x64/REDprelauncher.exe',
          label: 'Launcher',
          architecture: 'x64',
          preferred: false,
        },
      ],
      selectedExe: 'bin/x64/Cyberpunk2077.exe',
      anticheat: {
        detected: ['easyAntiCheat'],
        evidence: ['EasyAntiCheat/EasyAntiCheat_x64.dll'],
      },
    },
    tools: [],
    operation: null,
  };

  it('renders OptiScaler catalog card with GPL-3.0 badge and source repository link', () => {
    const sourceSpy = vi.spyOn(commands, 'openToolSource').mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('OptiScaler')).toBeInTheDocument();
    expect(screen.getByTestId('tool-license-badge')).toHaveTextContent('GPL-3.0');

    const sourceBtn = screen.getByTestId('tool-source-link');
    fireEvent.click(sourceBtn);
    expect(sourceSpy).toHaveBeenCalledWith('https://github.com/optiscaler/OptiScaler');
  });

  it('handles channel, version, target executable, and proxy DLL selections', () => {
    const channelSpy = vi.spyOn(commands, 'selectToolChannel').mockResolvedValue(undefined);
    const refreshSpy = vi.spyOn(commands, 'refreshToolVersions').mockResolvedValue(undefined);
    const versionSpy = vi.spyOn(commands, 'selectToolVersion').mockResolvedValue(undefined);
    const targetSpy = vi.spyOn(commands, 'selectToolTargetExe').mockResolvedValue(undefined);
    const proxySpy = vi.spyOn(commands, 'selectToolProxy').mockResolvedValue(undefined);
    const resetConfigSpy = vi.spyOn(commands, 'setToolResetConfig').mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    // Channel switch
    fireEvent.click(screen.getByTestId('channel-button-nightly'));
    expect(channelSpy).toHaveBeenCalledWith('nightly');

    // Refresh versions
    fireEvent.click(screen.getByTestId('refresh-versions-button'));
    expect(refreshSpy).toHaveBeenCalled();

    // Version select & cached indicator check
    expect(screen.getByRole('option', { name: /v0.7.2.*Cached/ })).toBeInTheDocument();
    fireEvent.change(screen.getByTestId('tool-version-select'), { target: { value: '0.7.1' } });
    expect(versionSpy).toHaveBeenCalledWith('0.7.1');

    // Target executable candidate selection
    expect(screen.getByRole('option', { name: /Cyberpunk2077.exe.*Recommended/ })).toBeInTheDocument();
    fireEvent.change(screen.getByTestId('target-exe-select'), { target: { value: 'bin/x64/REDprelauncher.exe' } });
    expect(targetSpy).toHaveBeenCalledWith('bin/x64/REDprelauncher.exe');

    // Proxy DLL selection
    expect(screen.getByTestId('proxy-dll-select')).toHaveValue('dxgi');
    fireEvent.change(screen.getByTestId('proxy-dll-select'), { target: { value: 'version' } });
    expect(proxySpy).toHaveBeenCalledWith('version');

    // Reset config checkbox
    fireEvent.click(screen.getByTestId('reset-config-checkbox'));
    expect(resetConfigSpy).toHaveBeenCalledWith(true);
  });

  it('reflects proxy selections returned by the backend', () => {
    const { rerender } = renderWithProviders(<GameToolsModal modal={baseModal} />);
    rerender(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={createAppQueryClient()}>
          <I18nProvider initialLocale="en">
            <GameToolsModal modal={{ ...baseModal, selectedProxy: 'winmm' }} />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    );
    expect(screen.getByTestId('proxy-dll-select')).toHaveValue('winmm');
    expect(screen.getByRole('option', { name: 'winmm.dll', selected: true })).toBeInTheDocument();
  });

  it('shows updating state on the action button', () => {
    const modal: ToolModalView = {
      ...baseModal,
      tools: [{ toolId: 'optiscaler', name: 'OptiScaler', channel: 'stable', version: '0.7.1',
        targetExe: 'game.exe', state: 'installed', summary: '' }],
      operation: { updating: { message: 'Updating OptiScaler', progressPercent: null } },
    };
    renderWithProviders(<GameToolsModal modal={modal} />);
    expect(screen.getByTestId('tool-update-button')).toBeDisabled();
    expect(screen.getByTestId('tool-update-button')).toHaveTextContent('Updating…');
    expect(screen.getByTestId('tool-operation-progress')).toHaveTextContent('Updating OptiScaler');
  });

  it('enforces anti-cheat warning confirmation gating on Install button', () => {
    const anticheatConfirmSpy = vi.spyOn(commands, 'setToolAnticheatConfirmed').mockResolvedValue(undefined);
    const installSpy = vi.spyOn(commands, 'installOrUpdateTool').mockResolvedValue(undefined);

    // Initial state: anti-cheat detected and unconfirmed -> Install button MUST be disabled
    const { rerender } = renderWithProviders(<GameToolsModal modal={baseModal} />);

    expect(screen.getByTestId('anticheat-warning-banner')).toBeInTheDocument();
    expect(screen.getByText(/EasyAntiCheat\/EasyAntiCheat_x64\.dll/)).toBeInTheDocument();

    const installBtn = screen.getByTestId('tool-install-button');
    expect(installBtn).toBeDisabled();

    // Check anti-cheat confirmation
    const confirmCheckbox = screen.getByTestId('anticheat-confirm-checkbox');
    expect(confirmCheckbox).not.toBeChecked();
    fireEvent.click(confirmCheckbox);
    expect(anticheatConfirmSpy).toHaveBeenCalledWith(true);

    // Rerender with anticheatConfirmed: true -> Install button enabled
    const confirmedModal: ToolModalView = {
      ...baseModal,
      anticheatConfirmed: true,
    };

    rerender(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={createAppQueryClient()}>
          <I18nProvider initialLocale="en">
            <GameToolsModal modal={confirmedModal} />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    );

    const enabledInstallBtn = screen.getByTestId('tool-install-button');
    expect(enabledInstallBtn).toBeEnabled();

    fireEvent.click(enabledInstallBtn);
    expect(installSpy).toHaveBeenCalledWith(false);
  });

  it('renders Update and Uninstall buttons when tool is installed, and triggers them', () => {
    const updateSpy = vi.spyOn(commands, 'installOrUpdateTool').mockResolvedValue(undefined);
    const uninstallSpy = vi.spyOn(commands, 'uninstallTool').mockResolvedValue(undefined);

    const installedModal: ToolModalView = {
      ...baseModal,
      anticheatConfirmed: true,
      tools: [
        {
          toolId: 'optiscaler',
          name: 'OptiScaler',
          channel: 'stable',
          version: '0.7.1',
          proxyDll: 'dxgi.dll',
          targetExe: 'bin/x64/Cyberpunk2077.exe',
          state: 'installed',
          summary: 'Installed version 0.7.1',
        },
      ],
    };

    renderWithProviders(<GameToolsModal modal={installedModal} />);

    expect(screen.queryByTestId('tool-install-button')).not.toBeInTheDocument();
    expect(screen.getByTestId('installed-version')).toHaveTextContent('Installed version: 0.7.1 (Stable)');

    const updateBtn = screen.getByTestId('tool-update-button');
    expect(updateBtn).toBeEnabled();
    fireEvent.click(updateBtn);
    expect(updateSpy).toHaveBeenCalledWith(true);

    const uninstallBtn = screen.getByTestId('tool-uninstall-button');
    expect(uninstallBtn).toBeEnabled();
    fireEvent.click(uninstallBtn);
    expect(uninstallSpy).toHaveBeenCalled();
  });

  it('shows live operation progress and failure error banners', () => {
    const installingModal: ToolModalView = {
      ...baseModal,
      operation: {
        installing: {
          progressPercent: 65,
          message: 'Extracting OptiScaler files',
        },
      },
    };

    const { rerender } = renderWithProviders(<GameToolsModal modal={installingModal} />);

    expect(screen.getByTestId('tool-operation-progress')).toHaveTextContent(
      'Extracting OptiScaler files (65%)',
    );
    expect(screen.getByTestId('tool-install-button')).toBeDisabled();
    expect(screen.getByTestId('tool-install-button')).toHaveTextContent('Installing…');
    expect(screen.getByTestId('tool-operation-progress').parentElement).toHaveAttribute(
      'aria-live',
      'polite',
    );

    // Failed operation state
    const failedModal: ToolModalView = {
      ...baseModal,
      operation: {
        failed: {
          message: 'Target executable locked by running process',
        },
      },
    };

    rerender(
      <MantineProvider theme={appTheme} defaultColorScheme="dark">
        <QueryClientProvider client={createAppQueryClient()}>
          <I18nProvider initialLocale="en">
            <GameToolsModal modal={failedModal} />
          </I18nProvider>
        </QueryClientProvider>
      </MantineProvider>,
    );

    expect(screen.getByTestId('tool-operation-error')).toHaveTextContent(
      'Target executable locked by running process',
    );
  });

  it('renders localized modal in pt-BR', () => {
    const installedModal: ToolModalView = {
      ...baseModal,
      anticheatConfirmed: true,
      tools: [
        {
          toolId: 'optiscaler',
          name: 'OptiScaler',
          channel: 'stable',
          version: '0.7.1',
          proxyDll: 'dxgi.dll',
          targetExe: 'bin/x64/Cyberpunk2077.exe',
          state: 'installed',
          summary: 'Installed version 0.7.1',
        },
      ],
    };

    renderWithProviders(<GameToolsModal modal={installedModal} />, undefined, 'pt-BR');

    expect(screen.getByText('Ferramentas do jogo')).toBeInTheDocument();
    expect(screen.getByTestId('installed-version')).toHaveTextContent('Versão instalada: 0.7.1 (Estável)');
    expect(screen.getByTestId('tool-update-button')).toHaveTextContent('Atualizar / Aplicar');
    expect(screen.getByTestId('tool-uninstall-button')).toHaveTextContent('Desinstalar');
    expect(screen.getByText('Versões disponíveis')).toBeInTheDocument();
    expect(screen.getByText('Nome da DLL proxy')).toBeInTheDocument();
    expect(screen.getByText('Atualizar versões')).toBeInTheDocument();
  });

  it('renders localized anti-cheat warning in pt-BR', () => {
    renderWithProviders(<GameToolsModal modal={baseModal} />, undefined, 'pt-BR');

    expect(screen.getByTestId('anticheat-warning-banner')).toBeInTheDocument();
    expect(screen.getByText(/Anti-cheat detectado \(easyAntiCheat\)/)).toBeInTheDocument();
    expect(screen.getByTestId('tool-install-button')).toHaveTextContent('Instalar OptiScaler');
  });

  it('renders when the backend reports no anti-cheat (empty detected list)', () => {
    // Backend serializes AntiCheatReport as { detected: [], evidence: [] } — no `systems` field.
    const cleanModal: ToolModalView = {
      ...baseModal,
      inspection: {
        ...baseModal.inspection!,
        anticheat: { detected: [], evidence: [] },
      },
    };

    renderWithProviders(<GameToolsModal modal={cleanModal} />);

    expect(screen.queryByTestId('anticheat-warning-banner')).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-install-button')).toBeEnabled();
  });

  it('renders the backend anti-cheat report shape without crashing', () => {
    const backendModal: ToolModalView = {
      ...baseModal,
      inspection: {
        ...baseModal.inspection!,
        anticheat: { detected: ['battlEye', 'blocklisted'], evidence: ['BattlEye/BEClient.dll'] },
      },
    };

    renderWithProviders(<GameToolsModal modal={backendModal} />);

    expect(screen.getByTestId('anticheat-warning-banner')).toHaveTextContent('battlEye, blocklisted');
    expect(screen.getByTestId('tool-install-button')).toBeDisabled();
  });
});
