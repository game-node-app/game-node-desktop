import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { I18nProvider } from '../../i18n';
import { ToastContainer } from '../settings/ToastContainer';
import { PLUGIN_NOTICE_EVENT, type PluginNotice } from '../../api';
import { resetUiStore, uiStore } from '../../store/uiStore';
import { usePluginNotices } from './usePluginNotices';

type Handler = (event: { payload: PluginNotice }) => void;
const listeners = new Map<string, Set<Handler>>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: Handler) => {
    if (!listeners.has(name)) listeners.set(name, new Set());
    listeners.get(name)!.add(handler);
    return () => listeners.get(name)?.delete(handler);
  }),
}));

function emit(payload: PluginNotice) {
  for (const handler of listeners.get(PLUGIN_NOTICE_EVENT) ?? []) handler({ payload });
}

const Probe: React.FC = () => {
  usePluginNotices();
  return null;
};

function renderProbe(locale: string) {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <Probe />
        <ToastContainer />
      </I18nProvider>
    </MantineProvider>,
  );
}

async function ready() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe('usePluginNotices', () => {
  beforeEach(() => {
    resetUiStore();
    listeners.clear();
  });
  afterEach(() => {
    listeners.clear();
  });

  it('shows a plugin notification attributed to the plugin, in the user language', async () => {
    renderProbe('pt-BR');
    await ready();
    act(() =>
      emit({
        pluginId: 'org.example.backup',
        severity: 'success',
        texts: [
          { locale: 'en', title: 'Backup finished', body: '3 files saved.' },
          { locale: 'pt-BR', title: 'Backup concluído', body: '3 arquivos salvos.' },
        ],
      }),
    );
    expect(screen.getByText('Backup concluído')).toBeInTheDocument();
    expect(screen.getByText('3 arquivos salvos.')).toBeInTheDocument();
    expect(screen.getByText('Do plugin org.example.backup')).toBeInTheDocument();
    expect(screen.queryByText('Backup finished')).toBeNull();
    expect(uiStore.state.toasts[0]).toMatchObject({ kind: 'success', source: 'org.example.backup' });
  });

  it('falls back to the plugin main text and maps severities to toast kinds', async () => {
    renderProbe('pt-BR');
    await ready();
    const send = (severity: PluginNotice['severity']) =>
      act(() =>
        emit({
          pluginId: 'org.example.plugin',
          severity,
          texts: [{ locale: 'en', title: `title ${severity}`, body: '' }],
        }),
      );
    send('info');
    send('success');
    send('warning');
    send('error');
    expect(uiStore.state.toasts.map((toast) => toast.kind)).toEqual([
      'info',
      'success',
      'warning',
      'danger',
    ]);
    expect(screen.getByText('title error')).toBeInTheDocument();
  });

  it('shows a plugin toast longer than Core\'s own', async () => {
    renderProbe('en');
    await ready();
    act(() => emit({ pluginId: 'p', severity: 'info', texts: [{ locale: '', title: 'T', body: '' }] }));
    expect(uiStore.state.toasts[0].durationMs).toBeGreaterThan(4000);
  });

  it('ignores a notification with nothing to show and survives a malformed one', async () => {
    renderProbe('en');
    await ready();
    act(() => emit({ pluginId: 'p', severity: 'info', texts: [{ locale: '', title: '  \u202e ', body: 'b' }] }));
    act(() => emit({ pluginId: 'p', severity: 'info' } as unknown as PluginNotice));
    act(() => emit({ pluginId: 'p', severity: 'nonsense' as never, texts: [{ locale: '', title: 'T', body: '' }] }));
    expect(uiStore.state.toasts).toHaveLength(1);
    expect(uiStore.state.toasts[0].kind).toBe('info');
  });

  it('stops listening when it unmounts', async () => {
    const view = renderProbe('en');
    await ready();
    expect(listeners.get(PLUGIN_NOTICE_EVENT)?.size).toBe(1);
    view.unmount();
    expect(listeners.get(PLUGIN_NOTICE_EVENT)?.size ?? 0).toBe(0);
  });
});
