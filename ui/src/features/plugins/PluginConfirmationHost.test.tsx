import { afterEach, beforeEach, describe, expect, it, vi, type MockInstance } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import {
  PLUGIN_CONFIRMATION_CLOSED_EVENT,
  PLUGIN_CONFIRMATION_EVENT,
  cleanConfirmation,
  commands,
  shortFingerprint,
  type PluginConfirmation,
} from '../../api';
import { I18nProvider } from '../../i18n';
import { PluginConfirmationHost } from './PluginConfirmationHost';

type Handler = (event: { payload: unknown }) => void;
const listeners = new Map<string, Set<Handler>>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(async (name: string, handler: Handler) => {
    if (!listeners.has(name)) listeners.set(name, new Set());
    listeners.get(name)!.add(handler);
    return () => listeners.get(name)?.delete(handler);
  }),
}));

function emit(name: string, payload: unknown) {
  for (const handler of listeners.get(name) ?? []) handler({ payload });
}

const FINGERPRINT = 'ab'.repeat(32);
const LAUNCH_PLAN = {
  sourceId: 'steam',
  requesterPluginId: 'org.example.launcher',
  requesterPublisher: FINGERPRINT,
  requesterPackageVersion: '1.0.0',
  target: 'C:\\Games\\Portal 2\\Portal2.exe',
  components: [],
  arguments: ['--profile', 'a profile & literal'],
  workingDirectory: 'C:\\Games\\Portal 2',
  environmentSet: { GAME_MODE: 'custom-value' },
  environmentUnset: ['OLD_MODE'],
  differences: ['recipe-selected', 'custom-target'],
  fingerprint: `v1:${'cd'.repeat(32)}`,
};

function confirmation(overrides: Partial<PluginConfirmation> = {}): PluginConfirmation {
  const operation = overrides.operation ?? 'launch';
  return {
    id: 'c1',
    pluginId: 'org.example.launcher',
    publisherFingerprint: FINGERPRINT,
    gameName: 'Space Game',
    operation,
    timeoutSeconds: 300,
    launchPlan: operation === 'launch' || operation === 'custom_launch' ? LAUNCH_PLAN : undefined,
    ...overrides,
  };
}

function renderHost(locale = 'en') {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <PluginConfirmationHost />
      </I18nProvider>
    </MantineProvider>,
  );
}

async function ready() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe('PluginConfirmationHost', () => {
  let answer: MockInstance<typeof commands.pluginConfirmationAnswer>;
  beforeEach(() => {
    listeners.clear();
    answer = vi.spyOn(commands, 'pluginConfirmationAnswer').mockResolvedValue(undefined);
  });
  afterEach(() => {
    listeners.clear();
    vi.restoreAllMocks();
  });

  it.each(['en', 'pt-BR'])('shows the complete launch review and masks values until revealed in %s', async (locale) => {
    renderHost(locale);
    await ready();
    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation()));
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveTextContent(locale === 'en' ? 'Launch plan' : 'Plano de lançamento');
    expect(dialog).toHaveTextContent(locale === 'en' ? 'Source' : 'Origem');
    expect(dialog).toHaveTextContent('--profile');
    expect(dialog).toHaveTextContent('a profile & literal');
    expect(dialog).toHaveTextContent(locale === 'en' ? 'Changes from the detected default' : 'Alterações em relação ao padrão detectado');
    expect(dialog).toHaveTextContent('••••••');
    expect(dialog).not.toHaveTextContent('custom-value');
    fireEvent.click(screen.getByRole('button', {
      name: locale === 'en' ? 'Reveal environment values' : 'Revelar valores do ambiente',
    }));
    expect(dialog).toHaveTextContent('custom-value');
    expect(screen.getByRole('checkbox').closest('label')).toHaveTextContent(locale === 'en'
      ? 'Remember approval for this exact launch plan'
      : 'Lembrar a aprovação deste plano de lançamento exato');
    fireEvent.click(screen.getByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', {
      name: locale === 'en' ? 'Run once' : 'Executar uma vez',
    }));
    expect(answer).toHaveBeenCalledWith('c1', true, true);
  });

  it.each(['en', 'pt-BR'])('reviews full restore targets without a remembered approval in %s', async (locale) => {
    renderHost(locale);
    await ready();
    const target = 'C:\\Saves\\<slot>.sav';
    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation({
      operation: 'backup_restore', details: ['Saves', 'snapshot-1', target], launchPlan: undefined,
    })));
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveTextContent(target);
    expect(dialog).toHaveTextContent('snapshot-1');
    expect(screen.queryByRole('checkbox')).not.toBeInTheDocument();
    const buttons = screen.getAllByRole('button');
    expect(buttons[0]).toHaveFocus();
    fireEvent.click(buttons[buttons.length - 1]);
    expect(answer).toHaveBeenCalledWith('c1', true, false);
  });

  it('asks about the game, naming the plugin and the publisher, with Cancel focused', async () => {
    renderHost();
    await ready();
    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation()));

    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveTextContent('Launch plan');
    expect(dialog).toHaveTextContent('Review the exact launch plan requested by org.example.launcher for Space Game.');
    expect(dialog).toHaveTextContent('ab:ab:ab:ab…ab:ab');
    expect(dialog).toHaveTextContent('does not certify');
    expect(dialog).toHaveTextContent('C:\\Games\\Portal 2\\Portal2.exe');
    expect(dialog).toHaveTextContent('--profile');
    expect(dialog).toHaveTextContent('••••••');
    expect(screen.getByRole('button', { name: 'Cancel' })).toHaveFocus();
    // Remembering is off unless the user turns it on.
    expect(screen.getByRole('checkbox')).not.toBeChecked();
    expect(screen.getByRole('checkbox').closest('label')).toHaveTextContent('Remember approval for this exact launch plan');
    fireEvent.click(screen.getByRole('button', { name: 'Reveal environment values' }));
    expect(dialog).toHaveTextContent('custom-value');
  });

  it('answers yes with what the user chose to remember, and no never remembers', async () => {
    renderHost();
    await ready();
    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation()));
    fireEvent.click(await screen.findByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'Run once' }));
    expect(answer).toHaveBeenCalledWith('c1', true, true);
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();

    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation({ id: 'c2' })));
    fireEvent.click(await screen.findByRole('checkbox'));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    // A refusal is never remembered, whatever the box says.
    expect(answer).toHaveBeenLastCalledWith('c2', false, false);
  });

  it('treats Escape as a refusal', async () => {
    renderHost();
    await ready();
    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation()));
    await screen.findByRole('alertdialog');
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Escape' });
    expect(answer).toHaveBeenCalledWith('c1', false, false);
  });

  it('queues requests, shows one at a time and forgets one the host stopped waiting for', async () => {
    renderHost();
    await ready();
    act(() => {
      emit(PLUGIN_CONFIRMATION_EVENT, confirmation({ id: 'c1' }));
      emit(PLUGIN_CONFIRMATION_EVENT, confirmation({ id: 'c1' })); // the same one again
      emit(PLUGIN_CONFIRMATION_EVENT, confirmation({ id: 'c2', gameName: 'Second Game', operation: 'install' }));
    });
    expect(await screen.findByText(/1 more waiting/)).toBeInTheDocument();
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Space Game');

    // The plugin withdrew the first request: the dialog moves on to the next one.
    act(() => emit(PLUGIN_CONFIRMATION_CLOSED_EVENT, 'c1'));
    expect(await screen.findByText('Let a plugin open an install flow?')).toBeInTheDocument();
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Second Game');
    expect(screen.queryByText(/more waiting/)).not.toBeInTheDocument();
    expect(answer).not.toHaveBeenCalled();

    act(() => emit(PLUGIN_CONFIRMATION_CLOSED_EVENT, 'c2'));
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();
  });

  it('shows what the plugin supplied as plain text and drops a malformed request', async () => {
    renderHost();
    await ready();
    act(() => {
      emit(PLUGIN_CONFIRMATION_EVENT, { id: 'bad', operation: 'format-disk', pluginId: 'x' });
      emit(PLUGIN_CONFIRMATION_EVENT, null);
    });
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument();

    act(() =>
      emit(
        PLUGIN_CONFIRMATION_EVENT,
        confirmation({ gameName: '<img src=x onerror=alert(1)>‮Evil\u0007\nGame' }),
      ),
    );
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog.querySelector('img')).toBeNull();
    expect(dialog).toHaveTextContent('<img src=x onerror=alert(1)>Evil Game');
    expect(dialog.textContent).not.toMatch(/[‮\u0007]/);
  });

  it('is localized in Portuguese', async () => {
    renderHost('pt-BR');
    await ready();
    act(() => emit(PLUGIN_CONFIRMATION_EVENT, confirmation({ operation: 'install' })));
    const dialog = await screen.findByRole('alertdialog');
    expect(dialog).toHaveTextContent('Permitir que um plugin abra um fluxo de instalação?');
    expect(dialog).toHaveTextContent('O plugin org.example.launcher quer abrir o fluxo de instalação de Space Game.');
    expect(screen.getByRole('button', { name: 'Permitir' })).toBeInTheDocument();
  });
});

describe('cleanConfirmation', () => {
  it('keeps a good request and bounds and cleans its text', () => {
    const cleaned = cleanConfirmation({
      ...confirmation(),
      pluginId: `org.${'x'.repeat(300)}`,
      gameName: 'g'.repeat(500),
      publisherFingerprint: 'not-a-fingerprint',
      timeoutSeconds: -4.7,
    });
    expect(cleaned?.pluginId.length).toBe(128);
    expect(cleaned?.gameName.length).toBe(120);
    expect(cleaned?.publisherFingerprint).toBe('');
    expect(cleaned?.timeoutSeconds).toBe(0);
  });

  it.each([
    [undefined],
    [42],
    [{ id: '', operation: 'launch', pluginId: 'x' }],
    [{ id: 'a', operation: 'delete', pluginId: 'x' }],
    [{ id: 'a', operation: 'launch', pluginId: '‮\u0007' }],
  ])('drops %j', (raw) => {
    expect(cleanConfirmation(raw)).toBeNull();
  });

  it('shortens only a real fingerprint', () => {
    expect(shortFingerprint(FINGERPRINT)).toBe('ab:ab:ab:ab…ab:ab');
    expect(shortFingerprint('abc')).toBe('');
  });
});
