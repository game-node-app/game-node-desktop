import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { appTheme, createAppQueryClient } from '../../../AppProviders';
import {
  APP_INFO_QUERY_KEY,
  LICENSE_NOTICES_QUERY_KEY,
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultSnapshot,
  type AppInfo,
} from '../../../api';
import { I18nProvider } from '../../../i18n';
import { openSettingsModal, resetUiStore } from '../../../store/uiStore';
import { ToastContainer } from '../ToastContainer';
import { SettingsModal } from '../SettingsModal';
import { AboutPanel } from './AboutPanel';

const APP_INFO: AppInfo = {
  locale: 'en',
  systemLocale: 'en',
  version: '0.9.0',
  buildDate: '2026-11-10',
  osVersion: 'Windows 11 Pro (build 26100)',
  arch: 'x64',
  backgroundSupported: true,
  platform: 'windows',
  logDirectory: '/tmp/gamenode-logs',
};

function makeClient(locale = 'en', appInfo: Partial<AppInfo> = {}) {
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, createDefaultSnapshot());
  queryClient.setQueryData(APP_INFO_QUERY_KEY, {
    ...APP_INFO,
    locale,
    systemLocale: locale,
    ...appInfo,
  });
  queryClient.setQueryData(
    LICENSE_NOTICES_QUERY_KEY,
    'Copyright 2026 GameNode Desktop Full License Text',
  );
  return queryClient;
}

function renderAbout(locale = 'en', appInfo: Partial<AppInfo> = {}) {
  const queryClient = makeClient(locale, appInfo);
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>
          <ToastContainer />
          <AboutPanel />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

function renderSettingsAbout(locale = 'en') {
  resetUiStore();
  openSettingsModal('about');
  const queryClient = makeClient(locale);
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>
          <ToastContainer />
          <SettingsModal />
        </I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

describe('AboutPanel', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    resetUiStore();
  });

  afterEach(() => {
    delete (navigator as unknown as { clipboard?: unknown }).clipboard;
  });

  it('shows the wordmark, installed version and build without an updater or channel', () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    renderAbout();

    expect(screen.getByAltText('GameNode')).toBeInTheDocument();
    expect(
      screen.getByText('GameNode Desktop 0.9.0'),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        'Build 2026-11-10 · Windows 11 Pro (build 26100) x64',
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Check for updates/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/channel/i)).not.toBeInTheDocument();
  });

  it('opens the recent log and the log folder from Diagnostics', async () => {
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    const openLogs = vi
      .spyOn(commands, 'openLogsFolder')
      .mockResolvedValue(undefined);
    vi.spyOn(commands, 'getRecentLogs').mockResolvedValue(
      '2026-11-10T00:00:00Z INFO gamenode_app: log line',
    );
    renderAbout();

    fireEvent.click(screen.getByRole('button', { name: 'Open log folder' }));
    expect(openLogs).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole('button', { name: 'Show recent log' }));
    expect(
      await screen.findByRole('dialog', { name: 'Recent log' }),
    ).toBeInTheDocument();
  });

  it('copies the redacted system information and confirms with a toast', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    const info = vi.spyOn(commands, 'systemInfoText').mockResolvedValue(
      [
        'GameNode Desktop 0.9.0',
        'Build 2026-11-10',
        'Windows 11 Pro (build 26100) x64',
        'Locale: en',
        '',
        'Plugins:',
        '- Steam 1.4.2 (official, installed)',
      ].join('\n'),
    );
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    renderAbout();

    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));

    await waitFor(() => expect(info).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining('GameNode Desktop 0.9.0'),
      ),
    );
    expect(await screen.findByText('Information copied')).toBeInTheDocument();
  });

  it('shows an error toast when the system information cannot be read', async () => {
    vi.spyOn(commands, 'systemInfoText').mockRejectedValue('io');
    vi.spyOn(commands, 'pluginInventory').mockResolvedValue([]);
    renderAbout();

    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));

    expect(
      await screen.findByText('Could not copy to the clipboard.'),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Copy' })).toBeEnabled();
  });

  it('opens the fixed GameNode links through the host command', () => {
    const openLink = vi
      .spyOn(commands, 'openGameNodeLink')
      .mockResolvedValue(undefined);
    renderAbout();

    fireEvent.click(screen.getByRole('button', { name: 'GameNode website' }));
    fireEvent.click(screen.getByRole('button', { name: 'Terms of use' }));
    fireEvent.click(screen.getByRole('button', { name: 'Privacy' }));
    fireEvent.click(screen.getByRole('button', { name: 'Report a problem' }));

    expect(openLink.mock.calls.map(([kind]) => kind)).toEqual([
      'website',
      'terms',
      'privacy',
      'reportIssue',
    ]);
  });

  it('opens license notices above Settings and restores focus when they close', async () => {
    const user = userEvent.setup();
    renderSettingsAbout();

    const trigger = screen.getByRole('button', {
      name: 'Show license notices',
    });
    await user.click(trigger);

    const licenseDialog = screen.getByRole('dialog', {
      name: 'License notices',
    });
    expect(licenseDialog).toBeInTheDocument();
    expect(
      screen.getByText('Copyright 2026 GameNode Desktop Full License Text'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('dialog', { name: 'Settings' }),
    ).toBeInTheDocument();

    await user.keyboard('{Escape}');

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'License notices' }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole('dialog', { name: 'Settings' }),
    ).toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('opens the recent log above Settings and returns focus to its button', async () => {
    const user = userEvent.setup();
    vi.spyOn(commands, 'getRecentLogs').mockResolvedValue(
      '2026-11-10T00:00:00Z INFO gamenode_app: log line',
    );
    renderSettingsAbout();

    const trigger = screen.getByRole('button', { name: 'Show recent log' });
    await user.click(trigger);

    expect(
      await screen.findByRole('dialog', { name: 'Recent log' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('dialog', { name: 'Settings' }),
    ).toBeInTheDocument();

    await user.keyboard('{Escape}');

    await waitFor(() =>
      expect(
        screen.queryByRole('dialog', { name: 'Recent log' }),
      ).not.toBeInTheDocument(),
    );
    expect(
      screen.getByRole('dialog', { name: 'Settings' }),
    ).toBeInTheDocument();
    await waitFor(() => expect(document.activeElement).toBe(trigger));
  });

  it('localizes the version, diagnostics and footer links in pt-BR', () => {
    renderAbout('pt-BR');

    const about = screen.getByRole('region', { name: 'Sobre' });
    expect(within(about).getByText('GameNode Desktop 0.9.0')).toBeInTheDocument();
    expect(within(about).getByText('Diagnóstico')).toBeInTheDocument();
    expect(within(about).getByText('Licenças')).toBeInTheDocument();
    expect(within(about).getByRole('button', { name: 'Site do GameNode' })).toBeInTheDocument();
    expect(within(about).getByRole('button', { name: 'Relatar um problema' })).toBeInTheDocument();
  });
});
