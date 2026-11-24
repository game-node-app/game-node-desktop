import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { commands } from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginFolderApprovalHost } from './PluginFolderApprovalHost';

type Handler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<Handler>>();
const openFolder = vi.fn();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: Handler) => {
    if (!listeners.has(name)) listeners.set(name, new Set());
    listeners.get(name)!.add(handler);
    return () => listeners.get(name)?.delete(handler);
  }),
}));

vi.mock('@tauri-apps/plugin-dialog', () => ({ open: (...args: unknown[]) => openFolder(...args) }));

function emit(name: string, payload: unknown) {
  for (const handler of listeners.get(name) ?? []) handler({ payload });
}

const FINGERPRINT = 'ab'.repeat(32);

function renderHost(locale = 'en') {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <PluginFolderApprovalHost />
      </I18nProvider>
    </MantineProvider>,
  );
}

async function ready() {
  await act(async () => { await Promise.resolve(); });
}

describe('PluginFolderApprovalHost', () => {
  beforeEach(() => {
    listeners.clear();
    openFolder.mockReset();
    vi.spyOn(commands, 'pluginFolderSelectionAnswer').mockResolvedValue(undefined);
    vi.spyOn(commands, 'pluginFolderApprovalAnswer').mockResolvedValue(undefined);
  });
  afterEach(() => {
    listeners.clear();
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
    vi.restoreAllMocks();
  });

  it.each([
    ['en', 'Choose a folder for a plugin'],
    ['pt-BR', 'Escolha uma pasta para o plugin'],
  ])('shows unverified suggestions and requires a second confirmation in %s', async (locale, title) => {
    renderHost(locale);
    await ready();
    act(() => emit('plugin:folder-approval-select', {
      id: '1234567890abcdef',
      pluginId: 'community.example',
      publisherFingerprint: FINGERPRINT,
      installationName: 'Space Game',
      kind: 'save',
      operations: ['read', 'write'],
      suggested: null,
      preselected: '/home/user/saves',
      previouslyApproved: { root: '/home/user/saves', operations: ['read'] },
      declared: [{ path: '/home/user/saves', description: 'Saves folder', exists: true }],
    }));
    expect(await screen.findByRole('heading', { name: title })).toBeInTheDocument();
    expect(screen.getAllByText((_content, element) => element?.textContent?.includes(locale === 'en' ? 'Declared by plugin' : 'Declarada pelo plugin') ?? false).length).toBeGreaterThan(0);
    expect(screen.getAllByText((_content, element) => element?.textContent?.includes(locale === 'en' ? 'Already approved: Read files' : 'Já aprovado: Ler arquivos') ?? false).length).toBeGreaterThan(0);
    fireEvent.change(screen.getByLabelText(locale === 'en' ? 'Selected folder path' : 'Caminho da pasta selecionada'), { target: { value: '/home/user/new-saves' } });
    fireEvent.click(screen.getByRole('button', { name: locale === 'en' ? 'Review folder' : 'Revisar pasta' }));
    expect(commands.pluginFolderSelectionAnswer).toHaveBeenCalledWith('1234567890abcdef', '/home/user/new-saves');

    act(() => emit('plugin:folder-approval-confirm', {
      id: 'abcdef0123456789',
      pluginId: 'community.example',
      publisherFingerprint: FINGERPRINT,
      installationName: 'Space Game',
      kind: 'save',
      operations: ['read', 'write'],
      root: '/home/user/new-saves',
      declaredByPlugin: false,
      affected: [{ name: 'Space Game', gameId: 17 }],
    }));
    expect(await screen.findByRole('heading', { name: locale === 'en' ? 'Approve this folder scope?' : 'Aprovar o acesso a esta pasta?' })).toBeInTheDocument();
    expect(screen.getByText('Space Game')).toBeInTheDocument();
    const cancel = screen.getAllByRole('button', { name: locale === 'en' ? 'Cancel' : 'Cancelar' })
      .find((button) => button.hasAttribute('data-autofocus'));
    expect(cancel).toHaveFocus();
    fireEvent.click(screen.getByRole('button', { name: locale === 'en' ? 'Approve folder' : 'Aprovar pasta' }));
    expect(commands.pluginFolderApprovalAnswer).toHaveBeenCalledWith('abcdef0123456789', true);
  });

  it('uses the native directory picker and explains logical library monitoring separately from Files access', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    openFolder.mockResolvedValue('/Games');
    renderHost();
    await ready();
    act(() => emit('plugin:folder-approval-select', {
      id: '1234567890abcdef', pluginId: 'community.example', publisherFingerprint: FINGERPRINT,
      installationName: 'My library', kind: 'library', operations: ['read', 'plugin_discovery', 'core_executable_discovery'],
      suggested: null, preselected: null, previouslyApproved: null, declared: [],
    }));
    fireEvent.click(await screen.findByRole('button', { name: 'Choose another folder…' }));
    expect(openFolder).toHaveBeenCalledWith({ directory: true, multiple: false, title: 'Choose a folder' });
    expect(await screen.findByText(/lets GameNode monitor the folder/)).toBeInTheDocument();
  });
});
