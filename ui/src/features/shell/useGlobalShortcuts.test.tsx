import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { commands } from '../../api/commands';
import { createDefaultFilters, createDefaultLibraryLayout } from '../../api/types';
import {
  openPalette,
  openSettingsModal,
  resetUiStore,
  uiStore,
} from '../../store/uiStore';
import { useGlobalShortcuts } from './useGlobalShortcuts';
import { makeLibrary, renderControls, setInputs } from '../library/controlsTestUtils';

function ShortcutHarness() {
  useGlobalShortcuts();
  return (
    <div>
      <input data-library-search aria-label="Library search" />
    </div>
  );
}

function renderShortcuts(library = makeLibrary()) {
  return renderControls(<ShortcutHarness />, library);
}

describe('useGlobalShortcuts', () => {
  beforeEach(() => {
    resetUiStore();
    vi.restoreAllMocks();
  });

  it('opens the palette with Ctrl+K, including from a text field', () => {
    renderShortcuts();
    const input = screen.getByLabelText('Library search');

    fireEvent.keyDown(document.body, { key: 'k', ctrlKey: true });
    expect(uiStore.state.paletteOpen).toBe(true);

    resetUiStore();
    fireEvent.keyDown(input, { key: 'k', ctrlKey: true });
    expect(uiStore.state.paletteOpen).toBe(true);
  });

  it('accepts Meta as the command modifier', () => {
    renderShortcuts();
    fireEvent.keyDown(document.body, { key: 'k', metaKey: true });
    expect(uiStore.state.paletteOpen).toBe(true);
  });

  it('opens Settings with Ctrl+, without disturbing a dirty draft or topic', () => {
    renderShortcuts();
    uiStore.setState((prev) => ({
      ...prev,
      settingsTopic: 'backups',
      backupDestinationDraft: '/tmp/backups',
      settingsOpen: true,
    }));

    fireEvent.keyDown(document.body, { key: ',', ctrlKey: true });
    expect(uiStore.state.settingsOpen).toBe(true);
    expect(uiStore.state.settingsTopic).toBe('backups');
    expect(uiStore.state.backupDestinationDraft).toBe('/tmp/backups');
  });

  it('rescans with F5 and quits with Ctrl+Q', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    const quit = vi.spyOn(commands, 'quitApp').mockResolvedValue();
    renderShortcuts();

    fireEvent.keyDown(document.body, { key: 'F5' });
    expect(rescan).toHaveBeenCalledTimes(1);

    fireEvent.keyDown(document.body, { key: 'q', ctrlKey: true });
    expect(quit).toHaveBeenCalledTimes(1);
  });

  it('focuses the library search with Ctrl+F and opens the filters with Ctrl+Shift+F', () => {
    renderShortcuts();
    const input = screen.getByLabelText('Library search');

    fireEvent.keyDown(document.body, { key: 'f', ctrlKey: true });
    expect(input).toHaveFocus();

    fireEvent.keyDown(document.body, { key: 'F', ctrlKey: true, shiftKey: true });
    expect(uiStore.state.filtersOpen).toBe(true);
  });

  it('toggles the view and the details panel through the engine input', () => {
    const input = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const library = makeLibrary({
      layout: {
        ...createDefaultLibraryLayout(),
        view: 'grid',
        detailOpen: true,
        filters: createDefaultFilters(),
      },
    });
    renderShortcuts(library);

    fireEvent.keyDown(document.body, { key: 'l', ctrlKey: true });
    fireEvent.keyDown(document.body, { key: 'i', ctrlKey: true });

    expect(setInputs(input)).toEqual([{ setView: 'list' }, { setDetailOpen: false }]);
  });

  it('opens the shortcuts dialog with ? outside text fields only', () => {
    renderShortcuts();
    const input = screen.getByLabelText('Library search');

    fireEvent.keyDown(input, { key: '?', shiftKey: true });
    expect(uiStore.state.shortcutsOpen).toBe(false);

    fireEvent.keyDown(document.body, { key: '?', shiftKey: true });
    expect(uiStore.state.shortcutsOpen).toBe(true);
  });

  it('does not run library shortcuts behind a deep dialog', () => {
    const input = vi.spyOn(commands, 'libraryInput').mockResolvedValue();
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    renderShortcuts();
    openSettingsModal('stores');

    fireEvent.keyDown(document.body, { key: 'f', ctrlKey: true });
    fireEvent.keyDown(document.body, { key: 'F', ctrlKey: true, shiftKey: true });
    fireEvent.keyDown(document.body, { key: 'l', ctrlKey: true });
    fireEvent.keyDown(document.body, { key: 'i', ctrlKey: true });
    fireEvent.keyDown(document.body, { key: 'F5' });
    fireEvent.keyDown(document.body, { key: '?' , shiftKey: true });
    fireEvent.keyDown(document.body, { key: 'k', ctrlKey: true });

    expect(input).not.toHaveBeenCalled();
    expect(rescan).not.toHaveBeenCalled();
    expect(uiStore.state.filtersOpen).toBe(false);
    expect(uiStore.state.shortcutsOpen).toBe(false);
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('ignores repeats and events another handler already consumed', () => {
    const rescan = vi.spyOn(commands, 'rescanLibrary').mockResolvedValue();
    renderShortcuts();

    fireEvent.keyDown(document.body, { key: 'F5', repeat: true });
    expect(rescan).not.toHaveBeenCalled();

    const event = new KeyboardEvent('keydown', {
      key: 'k',
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });
    event.preventDefault();
    document.body.dispatchEvent(event);
    expect(uiStore.state.paletteOpen).toBe(false);
  });

  it('keeps the palette trigger a single event above an open filter popover', () => {
    renderShortcuts();
    uiStore.setState((prev) => ({ ...prev, filtersOpen: true }));
    openPalette();
    fireEvent.keyDown(document.body, { key: 'k', ctrlKey: true });
    expect(uiStore.state.paletteOpen).toBe(true);
    expect(uiStore.state.filtersOpen).toBe(false);
  });
});
