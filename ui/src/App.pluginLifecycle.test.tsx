import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { App } from './App';
import { AppProviders } from './AppProviders';
import {
  createMockHost,
  MOCK_LIFECYCLE_PLUGIN_ID,
  MOCK_LIFECYCLE_PLUGIN_NAME,
} from './mock/host';
import { resetUiStore } from './store/uiStore';

type SnapshotEvent = { payload: unknown };
type SnapshotListener = (event: SnapshotEvent) => void;

/**
 * The UI's two host seams: `invoke` for commands and the `engine:snapshot` event
 * for live state. Both delegate to one in-memory mock host so every command,
 * publish and query behaves as it does in the desktop app.
 */
const bridge = vi.hoisted(() => ({
  host: null as null | {
    invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
  },
  listeners: new Map<string, Set<SnapshotListener>>(),
  dispose: null as null | (() => void),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    if (!bridge.host) throw new Error('the mock host is not ready');
    return bridge.host.invoke(cmd, args);
  },
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: async (event: string, listener: SnapshotListener) => {
    const listeners = bridge.listeners.get(event) ?? new Set<SnapshotListener>();
    listeners.add(listener);
    bridge.listeners.set(event, listeners);
    return () => {
      listeners.delete(listener);
    };
  },
}));

const pluginCard = () =>
  document.querySelector<HTMLElement>(`[data-plugin-id="${MOCK_LIFECYCLE_PLUGIN_ID}"]`);

const playtimeSection = () =>
  screen.getByRole('region', { name: 'Plugin playtime accounting' });

async function mountApp() {
  render(
    <AppProviders>
      <App />
    </AppProviders>,
  );
  await screen.findByTestId('sidebar');
}

async function openPluginsSettings() {
  fireEvent.click(
    within(screen.getByTestId('sidebar-settings-button')).getByRole('button'),
  );
  const nav = await screen.findByRole('navigation', { name: 'Settings' });
  fireEvent.click(within(nav).getByRole('button', { name: /^Plugins/ }));
}

async function closeSettings() {
  fireEvent.keyDown(window, { key: 'Escape' });
  await waitFor(() =>
    expect(screen.queryByRole('navigation', { name: 'Settings' })).not.toBeInTheDocument(),
  );
}

async function selectDolphinStore() {
  const shortcut = await screen.findByTestId('sidebar-provider-dolphin');
  expect(within(shortcut).getByText('2')).toBeInTheDocument();
  fireEvent.click(within(shortcut).getByRole('button', { name: 'Dolphin' }));
  await waitFor(() =>
    expect(screen.getByTestId('game-tile-dolphin:pikmin')).toBeInTheDocument(),
  );
  expect(screen.getByTestId('game-tile-dolphin:metroid')).toBeInTheDocument();
}

async function findPluginCard() {
  return waitFor(() => {
    const card = pluginCard();
    expect(card).not.toBeNull();
    return card as HTMLElement;
  });
}

describe('third-party plugin lifecycle in one mounted app', () => {
  beforeEach(() => {
    resetUiStore();
    bridge.listeners.clear();
    bridge.dispose?.();
    // Commands only use the IPC seam when the app sees a Tauri runtime.
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    const host = createMockHost({ pluginLifecycle: true, addBatchDelayMs: 0 });
    bridge.host = host;
    bridge.dispose = host.subscribe((snapshot) => {
      for (const listener of bridge.listeners.get('engine:snapshot') ?? []) {
        listener({ payload: snapshot });
      }
    });
  });

  afterEach(() => {
    bridge.dispose?.();
    bridge.dispose = null;
    bridge.host = null;
    bridge.listeners.clear();
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it('drops the store shortcut, its games and its playtime row on uninstall and restores them with the rescan count', async () => {
    await mountApp();
    await selectDolphinStore();

    await openPluginsSettings();
    await findPluginCard();
    expect(
      await within(playtimeSection()).findByRole('article', {
        name: MOCK_LIFECYCLE_PLUGIN_NAME,
      }),
    ).toBeInTheDocument();

    // Uninstall from the plugin card's details surface.
    fireEvent.click(within(await findPluginCard()).getByRole('button', { name: 'Details' }));
    const details = await screen.findByRole('article', { name: 'Plugin details' });
    fireEvent.click(within(details).getByRole('button', { name: 'Remove' }));
    const confirm = await screen.findByRole('dialog', { name: 'Remove' });
    fireEvent.click(within(confirm).getByRole('button', { name: 'Remove' }));

    // The live library stops containing the provider and its rows without a remount.
    await waitFor(() =>
      expect(screen.queryByTestId('sidebar-provider-dolphin')).not.toBeInTheDocument(),
    );
    await waitFor(() =>
      expect(screen.queryByTestId('game-tile-dolphin:pikmin')).not.toBeInTheDocument(),
    );
    expect(screen.queryByTestId('game-tile-dolphin:metroid')).not.toBeInTheDocument();
    await waitFor(() => expect(pluginCard()).toBeNull());
    await waitFor(() =>
      expect(
        within(playtimeSection()).queryByRole('article', {
          name: MOCK_LIFECYCLE_PLUGIN_NAME,
        }),
      ).not.toBeInTheDocument(),
    );

    // Reinstall through the visible import surface in the same session.
    const path = screen.getByRole('textbox', { name: 'Path to signed bundle' });
    fireEvent.change(path, { target: { value: '/plugins/dolphin.gnplugin' } });
    fireEvent.click(screen.getByRole('button', { name: 'Review package' }));
    const review = await screen.findByRole('article', { name: 'Review plugin package' });
    fireEvent.click(
      within(review).getByRole('button', { name: 'Trust this publisher' }),
    );
    await screen.findByText('Publisher trusted for this installation.');
    const trusted = await screen.findByRole('article', {
      name: 'Review plugin package',
    });
    fireEvent.click(
      within(trusted).getByRole('checkbox', {
        name: /I reviewed the declared native access/,
      }),
    );
    fireEvent.click(
      within(trusted).getByRole('button', { name: 'Accept, install and enable' }),
    );

    // The rescan adds a third game, so the shortcut count comes from the new view.
    await waitFor(() => expect(pluginCard()).not.toBeNull());
    expect(
      await within(playtimeSection()).findByRole('article', {
        name: MOCK_LIFECYCLE_PLUGIN_NAME,
      }),
    ).toBeInTheDocument();

    await closeSettings();
    const shortcut = await screen.findByTestId('sidebar-provider-dolphin');
    expect(within(shortcut).getByText('3')).toBeInTheDocument();
    expect(await screen.findByTestId('game-tile-dolphin:pikmin')).toBeInTheDocument();
    expect(screen.getByTestId('game-tile-dolphin:metroid')).toBeInTheDocument();
    expect(screen.getByTestId('game-tile-dolphin:fzero')).toBeInTheDocument();
  });

  it('marks a disabled plugin unavailable while its cached games stay visible', async () => {
    await mountApp();
    await selectDolphinStore();

    await openPluginsSettings();
    fireEvent.click(within(await findPluginCard()).getByRole('button', { name: 'Details' }));
    const details = await screen.findByRole('article', { name: 'Plugin details' });
    fireEvent.click(within(details).getByRole('button', { name: 'Disable' }));

    // Disabling stops the process but does not uninstall: the plugin card reports
    // unavailable and stopped, while the store shortcut and cached rows remain.
    await waitFor(() =>
      expect(within(pluginCard()!).getByText('Unavailable')).toBeInTheDocument(),
    );
    expect(
      within(screen.getByRole('article', { name: 'Plugin details' })).getByRole(
        'button',
        { name: 'Enable' },
      ),
    ).toBeInTheDocument();
    expect(screen.getByTestId('game-tile-dolphin:pikmin')).toBeInTheDocument();
    expect(screen.getByTestId('game-tile-dolphin:metroid')).toBeInTheDocument();
    expect(screen.getByTestId('sidebar-provider-dolphin')).toBeInTheDocument();
    expect(
      await within(playtimeSection()).findByRole('article', {
        name: MOCK_LIFECYCLE_PLUGIN_NAME,
      }),
    ).toBeInTheDocument();

    fireEvent.click(
      within(screen.getByRole('article', { name: 'Plugin details' })).getByRole(
        'button',
        { name: 'Enable' },
      ),
    );
    await waitFor(() =>
      expect(within(pluginCard()!).getByText('Session active')).toBeInTheDocument(),
    );
    expect(screen.getByTestId('game-tile-dolphin:pikmin')).toBeInTheDocument();

    await closeSettings();
    const shortcut = await screen.findByTestId('sidebar-provider-dolphin');
    expect(within(shortcut).getByText('2')).toBeInTheDocument();
  });
});
