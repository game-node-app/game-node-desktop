import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, render, screen, waitFor } from '@testing-library/react';
import { I18nProvider } from '../../i18n';
import { WindowControls } from './WindowControls';

const tauri = vi.hoisted(() => {
  const listeners: Array<() => void> = [];
  const state = { maximized: false };
  const api = {
    minimize: vi.fn().mockResolvedValue(undefined),
    toggleMaximize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    isMaximized: vi.fn(async () => state.maximized),
    onResized: vi.fn(async (handler: () => void) => {
      listeners.push(handler);
      return () => {
        const at = listeners.indexOf(handler);
        if (at >= 0) listeners.splice(at, 1);
      };
    }),
  };
  return {
    api,
    listeners,
    state,
    getCurrentWindow: vi.fn(() => api),
  };
});

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: tauri.getCurrentWindow,
}));

function renderControls() {
  return render(
    <I18nProvider initialLocale="en">
      <WindowControls />
    </I18nProvider>,
  );
}

describe('features/shell/WindowControls', () => {
  beforeEach(() => {
    tauri.listeners.length = 0;
    tauri.state.maximized = false;
    tauri.getCurrentWindow.mockClear();
    for (const action of [
      tauri.api.minimize,
      tauri.api.toggleMaximize,
      tauri.api.close,
      tauri.api.isMaximized,
      tauri.api.onResized,
    ]) {
      action.mockClear();
    }
    tauri.api.isMaximized.mockImplementation(async () => tauri.state.maximized);
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  });

  afterEach(() => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_IPC__;
  });

  it('minimizes, toggles maximize and closes through the Tauri window', async () => {
    renderControls();
    await waitFor(() => expect(tauri.api.isMaximized).toHaveBeenCalled());

    act(() => {
      screen.getByRole('button', { name: 'Minimize' }).click();
    });
    expect(tauri.api.minimize).toHaveBeenCalledTimes(1);

    act(() => {
      screen.getByRole('button', { name: 'Maximize' }).click();
    });
    expect(tauri.api.toggleMaximize).toHaveBeenCalledTimes(1);

    act(() => {
      screen.getByRole('button', { name: 'Close' }).click();
    });
    expect(tauri.api.close).toHaveBeenCalledTimes(1);
  });

  it('swaps the maximize control for Restore when a resize maximizes and back', async () => {
    renderControls();
    await waitFor(() => expect(tauri.api.onResized).toHaveBeenCalled());
    expect(screen.getByRole('button', { name: 'Maximize' })).toBeInTheDocument();

    tauri.state.maximized = true;
    act(() => {
      for (const listener of tauri.listeners) listener();
    });
    expect(
      await screen.findByRole('button', { name: 'Restore' }),
    ).toBeInTheDocument();

    tauri.state.maximized = false;
    act(() => {
      for (const listener of tauri.listeners) listener();
    });
    expect(
      await screen.findByRole('button', { name: 'Maximize' }),
    ).toBeInTheDocument();
  });

  it('renders the controls but calls no window APIs outside Tauri', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    delete (window as unknown as Record<string, unknown>).__TAURI_IPC__;
    renderControls();

    act(() => {
      screen.getByRole('button', { name: 'Minimize' }).click();
      screen.getByRole('button', { name: 'Maximize' }).click();
      screen.getByRole('button', { name: 'Close' }).click();
    });

    expect(tauri.getCurrentWindow).not.toHaveBeenCalled();
    expect(tauri.api.minimize).not.toHaveBeenCalled();
    expect(tauri.api.toggleMaximize).not.toHaveBeenCalled();
    expect(tauri.api.close).not.toHaveBeenCalled();
  });

  it('localizes the control labels', async () => {
    render(
      <I18nProvider initialLocale="pt-BR">
        <WindowControls />
      </I18nProvider>,
    );
    expect(
      screen.getByRole('button', { name: 'Minimizar' }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Maximizar' }),
    ).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Fechar' })).toBeInTheDocument();
  });
});
