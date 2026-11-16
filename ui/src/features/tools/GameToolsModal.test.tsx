import { describe, expect, it, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { QueryClientProvider } from '@tanstack/react-query';
import { MantineProvider } from '@mantine/core';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  commands,
  createDefaultSnapshot,
  SNAPSHOT_QUERY_KEY,
  type Snapshot,
  type ToolModalView,
  type ToolRelease,
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

  const wrap = (element: React.ReactElement) => (
    <MantineProvider theme={appTheme} defaultColorScheme="dark" env="test">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{element}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>
  );

  const result = render(wrap(ui));
  return {
    ...result,
    rerender: (next: React.ReactElement) => result.rerender(wrap(next)),
  };
}

const baseRelease: ToolRelease = {
  toolId: 'optiscaler',
  channel: 'stable',
  version: '0.7.2',
  tagName: 'v0.7.2',
  name: 'OptiScaler 0.7.2',
  publishedAt: '2026-05-01',
  downloadUrl:
    'https://github.com/optiscaler/OptiScaler/releases/download/v0.7.2/OptiScaler.7z',
  assetName: 'OptiScaler.7z',
  sizeBytes: 39845888,
  sha256: 'abc123',
  cachedPath: null,
};

const olderRelease: ToolRelease = {
  toolId: 'optiscaler',
  channel: 'stable',
  version: '0.7.1',
  tagName: 'v0.7.1',
  name: 'OptiScaler 0.7.1',
  publishedAt: '2026-04-10',
  downloadUrl:
    'https://github.com/optiscaler/OptiScaler/releases/download/v0.7.1/OptiScaler.7z',
  assetName: 'OptiScaler.7z',
  sizeBytes: 14500000,
};

const baseModal: ToolModalView = {
  entryKey: 'steam:1091500',
  title: 'Cyberpunk 2077',
  installDir: 'D:\\Steam\\steamapps\\common\\Cyberpunk2077',
  activeToolId: 'optiscaler',
  selectedChannel: 'stable',
  availableVersions: [baseRelease, olderRelease],
  selectedVersion: '0.7.2',
  selectedProxy: 'dxgi',
  anticheatConfirmed: false,
  inspection: {
    entryKey: 'steam:1091500',
    installDir: 'D:\\Steam\\steamapps\\common\\Cyberpunk2077',
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

function installedModal(overrides: Partial<ToolModalView> = {}): ToolModalView {
  return {
    ...baseModal,
    anticheatConfirmed: true,
    tools: [
      {
        toolId: 'optiscaler',
        name: 'OptiScaler',
        channel: 'stable',
        version: '0.7.2',
        proxyDll: 'dxgi.dll',
        targetExe: 'bin/x64/Cyberpunk2077.exe',
        state: 'installed',
        summary: 'Installed version 0.7.2',
      },
    ],
    ...overrides,
  };
}

describe('GameToolsModal', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('renders the tool card with its status pill, license and source link', () => {
    const sourceSpy = vi
      .spyOn(commands, 'openToolSource')
      .mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    expect(screen.getByRole('dialog')).toBeInTheDocument();
    expect(screen.getByText('OptiScaler')).toBeInTheDocument();
    expect(screen.getByTestId('tool-license-badge')).toHaveTextContent(
      'GPL-3.0',
    );
    expect(screen.getByText('Not installed')).toBeInTheDocument();
    expect(
      screen.getByText('Cyberpunk 2077 · Steam'),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByTestId('tool-source-link'));
    expect(sourceSpy).toHaveBeenCalledWith(
      'https://github.com/optiscaler/OptiScaler',
    );
  });

  it('switches channel and refreshes the available versions', async () => {
    const user = userEvent.setup();
    const channelSpy = vi
      .spyOn(commands, 'selectToolChannel')
      .mockResolvedValue(undefined);
    const refreshSpy = vi
      .spyOn(commands, 'refreshToolVersions')
      .mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    expect(screen.getByRole('radio', { name: 'Stable' })).toBeChecked();
    await user.click(screen.getByRole('radio', { name: 'Nightly' }));
    expect(channelSpy).toHaveBeenCalledWith('nightly');

    await user.click(screen.getByTestId('refresh-versions-button'));
    expect(refreshSpy).toHaveBeenCalled();
  });

  it('selects a version and a proxy DLL from their shared select fields', async () => {
    const user = userEvent.setup();
    const versionSpy = vi
      .spyOn(commands, 'selectToolVersion')
      .mockResolvedValue(undefined);
    const proxySpy = vi
      .spyOn(commands, 'selectToolProxy')
      .mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    const version = screen.getByRole('combobox', { name: 'Version' });
    expect(version).toHaveTextContent('0.7.2 (recommended)');
    await user.click(version);
    await user.click(
      await screen.findByRole('option', { name: '0.7.1' }),
    );
    expect(versionSpy).toHaveBeenCalledWith('0.7.1');

    const proxy = screen.getByRole('combobox', { name: 'Proxy DLL name' });
    expect(proxy).toHaveTextContent('dxgi.dll');
    await user.click(proxy);
    await user.click(
      await screen.findByRole('option', { name: 'version.dll' }),
    );
    expect(proxySpy).toHaveBeenCalledWith('version');
  });

  it('shows the read-only target and calls selectToolTargetExe from a candidate radio', async () => {
    const user = userEvent.setup();
    const targetSpy = vi
      .spyOn(commands, 'selectToolTargetExe')
      .mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    const target = screen.getByTestId('target-exe-input');
    expect(target).toHaveAttribute('readonly');
    expect(target).toHaveValue(
      'D:\\Steam\\steamapps\\common\\Cyberpunk2077\\bin\\x64\\Cyberpunk2077.exe',
    );

    const selected = screen.getByRole('radio', {
      name: 'bin/x64/Cyberpunk2077.exe',
    });
    expect(selected).toBeChecked();

    const launcher = screen.getByRole('radio', {
      name: 'bin/x64/REDprelauncher.exe (launcher)',
    });
    await user.click(launcher);
    expect(targetSpy).toHaveBeenCalledWith('bin/x64/REDprelauncher.exe');
  });

  it('opens the executable picker from Browse and toggles the reset-config checkbox', () => {
    const pickSpy = vi
      .spyOn(commands, 'pickToolTargetExe')
      .mockResolvedValue(undefined);
    const openFolderSpy = vi
      .spyOn(commands, 'openInstallFolder')
      .mockResolvedValue(undefined);
    const resetSpy = vi
      .spyOn(commands, 'setToolResetConfig')
      .mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={baseModal} />);

    fireEvent.click(screen.getByTestId('tool-browse-button'));
    expect(pickSpy).toHaveBeenCalledOnce();
    expect(openFolderSpy).not.toHaveBeenCalled();

    const reset = screen.getByTestId('reset-config-checkbox');
    expect(reset).not.toBeChecked();
    fireEvent.click(reset);
    expect(resetSpy).toHaveBeenCalledWith(true);
  });

  it('keeps Install disabled until the anti-cheat risk is confirmed', () => {
    const confirmSpy = vi
      .spyOn(commands, 'setToolAnticheatConfirmed')
      .mockResolvedValue(undefined);
    const installSpy = vi
      .spyOn(commands, 'installOrUpdateTool')
      .mockResolvedValue(undefined);

    const { rerender } = renderWithProviders(
      <GameToolsModal modal={baseModal} />,
    );

    const banner = screen.getByTestId('anticheat-warning-banner');
    expect(banner).toHaveTextContent('Anti-cheat detected (Easy Anti-Cheat)');
    expect(banner).toHaveTextContent('EasyAntiCheat/EasyAntiCheat_x64.dll');

    const install = screen.getByTestId('tool-install-button');
    expect(install).toBeDisabled();

    const confirm = screen.getByTestId('anticheat-confirm-checkbox');
    expect(confirm).not.toBeChecked();
    fireEvent.click(confirm);
    expect(confirmSpy).toHaveBeenCalledWith(true);

    rerender(
      <GameToolsModal
        modal={{ ...baseModal, anticheatConfirmed: true }}
      />,
    );

    const enabled = screen.getByTestId('tool-install-button');
    expect(enabled).toBeEnabled();
    fireEvent.click(enabled);
    expect(installSpy).toHaveBeenCalledWith(false);
  });

  it('omits the anti-cheat callout and keeps Install enabled on a clean game', () => {
    const cleanModal: ToolModalView = {
      ...baseModal,
      inspection: {
        ...baseModal.inspection!,
        anticheat: { detected: [], evidence: [] },
      },
    };

    renderWithProviders(<GameToolsModal modal={cleanModal} />);

    expect(
      screen.queryByTestId('anticheat-warning-banner'),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId('tool-install-button')).toBeEnabled();
  });

  it('renders the footer download note variants', () => {
    const { rerender } = renderWithProviders(
      <GameToolsModal modal={baseModal} />,
    );
    expect(screen.getByTestId('tool-download-note')).toHaveTextContent(
      '38 MB download, verified with SHA-256',
    );

    rerender(
      <GameToolsModal
        modal={{
          ...baseModal,
          availableVersions: [{ ...baseRelease, sha256: null }],
        }}
      />,
    );
    expect(screen.getByTestId('tool-download-note')).toHaveTextContent(
      '38 MB download',
    );

    rerender(
      <GameToolsModal
        modal={{
          ...baseModal,
          availableVersions: [
            { ...baseRelease, cachedPath: '/cache/OptiScaler.7z' },
          ],
        }}
      />,
    );
    expect(screen.getByTestId('tool-download-note')).toHaveTextContent(
      'Already downloaded, verified with SHA-256',
    );

    rerender(
      <GameToolsModal
        modal={{
          ...baseModal,
          availableVersions: [
            {
              ...baseRelease,
              sha256: null,
              cachedPath: '/cache/OptiScaler.7z',
            },
          ],
        }}
      />,
    );
    expect(screen.getByTestId('tool-download-note')).toHaveTextContent(
      'Already downloaded',
    );
  });

  it('renders installed, update and uninstall actions', () => {
    const updateSpy = vi
      .spyOn(commands, 'installOrUpdateTool')
      .mockResolvedValue(undefined);
    const uninstallSpy = vi
      .spyOn(commands, 'uninstallTool')
      .mockResolvedValue(undefined);

    renderWithProviders(<GameToolsModal modal={installedModal()} />);

    expect(screen.queryByTestId('tool-install-button')).not.toBeInTheDocument();
    expect(screen.getByTestId('installed-version')).toHaveTextContent(
      'Installed · 0.7.2',
    );

    const update = screen.getByTestId('tool-update-button');
    expect(update).toHaveTextContent('Update OptiScaler');
    expect(update).toBeEnabled();
    fireEvent.click(update);
    expect(updateSpy).toHaveBeenCalledWith(true);

    const uninstall = screen.getByTestId('tool-uninstall-button');
    expect(uninstall).toBeEnabled();
    fireEvent.click(uninstall);
    expect(uninstallSpy).toHaveBeenCalled();
  });

  it('shows Update available when the installed version is behind the latest release', () => {
    renderWithProviders(
      <GameToolsModal
        modal={installedModal({
          tools: [
            {
              toolId: 'optiscaler',
              name: 'OptiScaler',
              channel: 'stable',
              version: '0.7.0',
              proxyDll: 'dxgi.dll',
              targetExe: 'bin/x64/Cyberpunk2077.exe',
              state: 'installed',
              summary: '',
            },
          ],
        })}
      />,
    );

    expect(screen.getByTestId('installed-version')).toHaveTextContent(
      'Update available',
    );
  });

  it('shows live operation progress and failure banners', () => {
    const { rerender } = renderWithProviders(
      <GameToolsModal
        modal={{
          ...baseModal,
          operation: {
            installing: {
              progressPercent: 65,
              message: 'Extracting OptiScaler files',
            },
          },
        }}
      />,
    );

    expect(screen.getByTestId('tool-operation-progress')).toHaveTextContent(
      'Extracting OptiScaler files (65%)',
    );
    expect(screen.getByTestId('tool-install-button')).toBeDisabled();
    expect(screen.getByTestId('tool-install-button')).toHaveTextContent(
      'Installing…',
    );
    expect(
      screen.getByTestId('tool-operation-progress').parentElement,
    ).toHaveAttribute('aria-live', 'polite');

    rerender(
      <GameToolsModal
        modal={{
          ...baseModal,
          operation: {
            failed: {
              message: 'Target executable locked by running process',
            },
          },
        }}
      />,
    );

    expect(screen.getByTestId('tool-operation-error')).toHaveTextContent(
      'Target executable locked by running process',
    );
  });

  it('hides the candidate list when inspection finds no executables', () => {
    renderWithProviders(
      <GameToolsModal
        modal={{
          ...baseModal,
          inspection: {
            ...baseModal.inspection!,
            candidates: [],
            selectedExe: null,
          },
        }}
      />,
    );

    expect(
      screen.queryByRole('radiogroup', {
        name: /Detected candidate executables/,
      }),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId('target-exe-input')).toHaveValue('');
    expect(
      screen.getByPlaceholderText('No executable selected'),
    ).toBeInTheDocument();
  });

  it('localizes the dialog in pt-BR', () => {
    renderWithProviders(
      <GameToolsModal modal={installedModal()} />,
      undefined,
      'pt-BR',
    );

    expect(screen.getByText('Ferramentas do jogo')).toBeInTheDocument();
    expect(
      screen.getByText('Cyberpunk 2077 · Steam'),
    ).toBeInTheDocument();
    expect(screen.getByTestId('installed-version')).toHaveTextContent(
      'Instalado · 0.7.2',
    );
    expect(screen.getByText('Canal')).toBeInTheDocument();
    expect(screen.getByText('Versão')).toBeInTheDocument();
    expect(screen.getByText('Nome da DLL proxy')).toBeInTheDocument();
    expect(screen.getByText('Executável de destino')).toBeInTheDocument();
    expect(screen.getByText('Procurar…')).toBeInTheDocument();
    expect(
      screen.getByRole('radio', {
        name: 'bin/x64/REDprelauncher.exe (inicializador)',
      }),
    ).toBeInTheDocument();
    expect(screen.getByTestId('tool-download-note')).toHaveTextContent(
      'Download de 38 MB, verificado por SHA-256',
    );
    expect(screen.getByTestId('tool-cancel-button')).toHaveTextContent(
      'Cancelar',
    );
    expect(screen.getByTestId('tool-uninstall-button')).toHaveTextContent(
      'Desinstalar',
    );
    expect(screen.getByTestId('tool-update-button')).toHaveTextContent(
      'Atualizar OptiScaler',
    );
  });

  it('localizes the anti-cheat callout in pt-BR', () => {
    renderWithProviders(<GameToolsModal modal={baseModal} />, undefined, 'pt-BR');

    expect(screen.getByTestId('anticheat-warning-banner')).toHaveTextContent(
      'Anti-cheat detectado (Easy Anti-Cheat)',
    );
    expect(screen.getByTestId('tool-install-button')).toHaveTextContent(
      'Instalar OptiScaler',
    );
    expect(screen.getByTestId('tool-install-button')).toBeDisabled();
  });

  it('disables Browse while a tool operation is running and localizes picker errors', () => {
    const { rerender } = renderWithProviders(
      <GameToolsModal
        modal={{
          ...baseModal,
          operation: {
            installing: {
              progressPercent: 40,
              message: 'Installing',
            },
          },
        }}
      />,
    );
    expect(screen.getByTestId('tool-browse-button')).toBeDisabled();

    rerender(
      <GameToolsModal
        modal={{
          ...baseModal,
          operation: {
            failed: {
              code: 'targetOutsideInstallDir',
              message: 'Target executable path escapes the game directory',
            },
          },
        }}
      />,
    );
    expect(screen.getByTestId('tool-operation-error')).toHaveTextContent(
      "The selected executable must be inside this game's install folder.",
    );

    renderWithProviders(
      <GameToolsModal
        modal={{
          ...baseModal,
          operation: {
            failed: {
              code: 'pickerFailed',
              message: 'The executable picker could not open.',
            },
          },
        }}
      />,
      undefined,
      'pt-BR',
    );
    expect(screen.getAllByTestId('tool-operation-error').at(-1)).toHaveTextContent(
      'Não foi possível abrir o seletor de executável.',
    );
  });
});
