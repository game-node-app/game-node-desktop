import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginRuntimeApprovalHost } from './PluginRuntimeApprovalHost';

type Handler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<Handler>>();
const openFile = vi.fn();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: Handler) => {
    if (!listeners.has(name)) listeners.set(name, new Set());
    listeners.get(name)!.add(handler);
    return () => listeners.get(name)?.delete(handler);
  }),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...args: unknown[]) => openFile(...args) }));

function emit(name: string, payload: unknown) {
  for (const handler of listeners.get(name) ?? []) handler({ payload });
}

const FINGERPRINT = 'ab'.repeat(32);

function renderHost(locale = 'en') {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <PluginRuntimeApprovalHost />
      </I18nProvider>
    </MantineProvider>,
  );
}

async function ready() {
  await act(async () => { await Promise.resolve(); });
}

function prompt() {
  return {
    id: '1234567890abcdef',
    pluginId: 'org.gamenode.plugin.dolphin',
    publisherFingerprint: FINGERPRINT,
    runtimeName: 'Dolphin Emulator',
    runtimeVersion: '2503',
    platform: 'linux',
    supportedFormats: ['gamecube-rom', 'wii-rom'],
    hints: ['dolphin-emu', 'dolphin-emu.desktop'],
    candidates: [{
      displayName: 'Dolphin Emulator',
      packageManager: 'pacman',
      packageId: 'dolphin-emu;2503-1;x86_64;extra',
      version: '2503-1',
      path: '/usr/bin/dolphin-emu',
    }],
  };
}

describe('PluginRuntimeApprovalHost', () => {
  beforeEach(() => {
    listeners.clear();
    openFile.mockReset();
    vi.spyOn(commands, 'pluginRuntimeSelectionAnswer').mockResolvedValue(undefined);
  });
  afterEach(() => {
    listeners.clear();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.restoreAllMocks();
  });

  it.each([
    ['en', 'Choose a program for a plugin', 'Approve program'],
    ['pt-BR', 'Escolha um programa para o plugin', 'Aprovar programa'],
  ])('shows the publisher, package and exact path and approves only the chosen program in %s', async (locale, title, approve) => {
    renderHost(locale);
    await ready();
    act(() => emit('plugin:runtime-approval-select', prompt()));
    expect(await screen.findByRole('heading', { name: title })).toBeInTheDocument();
    expect(screen.getByText(/org\.gamenode\.plugin\.dolphin/)).toBeInTheDocument();
    expect(screen.getByText(/pacman · dolphin-emu;2503-1;x86_64;extra · 2503-1/)).toBeInTheDocument();
    expect(screen.getByDisplayValue('/usr/bin/dolphin-emu')).toBeInTheDocument();
    const cancel = screen.getAllByRole('button', { name: locale === 'en' ? 'Cancel' : 'Cancelar' })
      .find((button) => button.hasAttribute('data-autofocus'));
    expect(cancel).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: approve }));
    expect(commands.pluginRuntimeSelectionAnswer).toHaveBeenCalledWith('1234567890abcdef', '/usr/bin/dolphin-emu');
  });

  it('uses the native file picker and only sends the path the user selected', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    openFile.mockResolvedValue('/opt/emulators/dolphin-emu');
    renderHost();
    await ready();
    act(() => emit('plugin:runtime-approval-select', prompt()));
    fireEvent.click(await screen.findByRole('button', { name: 'Browse…' }));
    expect(openFile).toHaveBeenCalledWith({ directory: false, multiple: false, title: 'Choose the program to approve' });
    expect(await screen.findByDisplayValue('/opt/emulators/dolphin-emu')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Approve program' }));
    expect(commands.pluginRuntimeSelectionAnswer).toHaveBeenCalledWith('1234567890abcdef', '/opt/emulators/dolphin-emu');
  });

  it('cancels with a null answer and a closed prompt disappears', async () => {
    renderHost();
    await ready();
    act(() => emit('plugin:runtime-approval-select', prompt()));
    const cancel = (await screen.findAllByRole('button', { name: 'Cancel' }))
      .find((button) => button.hasAttribute('data-autofocus'))!;
    fireEvent.click(cancel);
    expect(commands.pluginRuntimeSelectionAnswer).toHaveBeenCalledWith('1234567890abcdef', null);
    await ready();

    act(() => emit('plugin:runtime-approval-select', prompt()));
    expect(await screen.findByRole('heading', { name: 'Choose a program for a plugin' })).toBeInTheDocument();
    act(() => emit('plugin:runtime-approval-closed', '1234567890abcdef'));
    expect(screen.queryByRole('heading', { name: 'Choose a program for a plugin' })).not.toBeInTheDocument();
  });

  it('shows manual browse when the inventory has no candidates', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    renderHost();
    await ready();
    act(() => emit('plugin:runtime-approval-select', {
      ...prompt(),
      candidates: [],
    }));
    expect(await screen.findByText(/No matching installed program/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Browse…' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Approve program' })).toBeDisabled();
  });
});
