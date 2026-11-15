import { useEffect, useRef } from 'react';
import { commands } from '../../api/commands';
import { useLibraryView } from '../../api/useSnapshot';
import type { LibraryView } from '../../api/types';
import {
  openFilters,
  openPalette,
  openSettingsModal,
  openShortcuts,
  setFiltersOpen,
  uiStore,
} from '../../store/uiStore';
import { isShortcutBlockedByOverlay, isTextEntryTarget } from '../library/libraryShortcuts';

/** The attribute the library search input publishes for the global Ctrl+F. */
export const LIBRARY_SEARCH_ATTRIBUTE = 'data-library-search';

function focusLibrarySearch(): void {
  const input = document.querySelector<HTMLInputElement>(
    `[${LIBRARY_SEARCH_ATTRIBUTE}]`,
  );
  if (!input) return;
  input.focus();
  input.select();
}

/**
 * Surfaces Ctrl+K must not open the palette over. The app menu and the filter popover are
 * not included: the palette replaces them instead of stacking on top, and `openPalette`
 * already dismisses the app menu.
 */
function isDeepOverlayOpen(library?: LibraryView | null): boolean {
  const state = uiStore.state;
  if (
    state.settingsOpen ||
    state.licenseModalOpen ||
    state.shortcutsOpen ||
    state.saveViewOpen ||
    state.contextMenu.open ||
    state.discoveryModalOpen ||
    state.localEditorEntryKey !== null ||
    state.saveProtectionEntryKey !== null ||
    state.removeModalEntryKey !== null ||
    state.backupDestinationDraft !== null ||
    state.discardBackupConfirmOpen
  ) {
    return true;
  }
  return Boolean(library?.addDialog || library?.matchDialog);
}

/**
 * The single global keyboard layer (design D11), mounted once in `App`. It keeps every
 * shortcut that is not a library-only key:
 *
 * - Ctrl/Cmd+K, Ctrl/Cmd+,, F5, Ctrl/Cmd+Q;
 * - Ctrl/Cmd+F, Ctrl/Cmd+Shift+F, Ctrl/Cmd+L, Ctrl/Cmd+I;
 * - `?` outside text fields.
 *
 * Library-only keys (F, Enter, arrows, Ctrl+A, Shift+S, Shift+F10 and clearing the
 * selection with Esc) stay in `LibraryScreen`'s focused container so they never fire over
 * a dialog. Text fields swallow the unmodified keys; Ctrl/Cmd combinations still work and
 * are handled exactly once with `preventDefault`.
 *
 * The hook replaces the old settings-only shortcut listener. Settings state is
 * only touched by the open helpers, so opening Settings again while its backups draft is
 * dirty keeps the draft and the discard guard.
 */
export function useGlobalShortcuts(): void {
  const library = useLibraryView();
  const libraryRef = useRef<LibraryView | null | undefined>(library);
  libraryRef.current = library;

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || event.isComposing || event.repeat) return;

      const library = libraryRef.current;
      const command = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();

      if (command && !event.altKey) {
        if (key === 'k' && !event.shiftKey) {
          if (isDeepOverlayOpen(library)) return;
          event.preventDefault();
          // The palette owns the foreground; a filter popover would otherwise stay
          // stacked behind it with its own keyboard scope.
          setFiltersOpen(false);
          openPalette();
          return;
        }

        if (event.key === ',' && !event.shiftKey) {
          event.preventDefault();
          openSettingsModal();
          return;
        }

        if (key === 'f' && event.shiftKey) {
          if (!library || isShortcutBlockedByOverlay(library, { allowFiltersOpen: true })) {
            return;
          }
          event.preventDefault();
          openFilters();
          return;
        }

        if (key === 'f' && !event.shiftKey) {
          if (isShortcutBlockedByOverlay(library)) return;
          event.preventDefault();
          focusLibrarySearch();
          return;
        }

        if (key === 'l' && !event.shiftKey) {
          if (!library || isShortcutBlockedByOverlay(library)) return;
          event.preventDefault();
          void commands.libraryInput({
            setView: library.layout.view === 'grid' ? 'list' : 'grid',
          });
          return;
        }

        if (key === 'i' && !event.shiftKey) {
          if (!library || isShortcutBlockedByOverlay(library)) return;
          event.preventDefault();
          void commands.libraryInput({ setDetailOpen: !library.layout.detailOpen });
          return;
        }

        if (key === 'q' && !event.shiftKey) {
          event.preventDefault();
          void commands.quitApp();
        }
        return;
      }

      if (!event.ctrlKey && !event.metaKey && !event.altKey && event.key === '?') {
        if (isTextEntryTarget(event.target) || isShortcutBlockedByOverlay(library)) return;
        event.preventDefault();
        openShortcuts();
        return;
      }

      if (event.key === 'F5' && !event.ctrlKey && !event.metaKey && !event.altKey) {
        if (isShortcutBlockedByOverlay(library)) return;
        event.preventDefault();
        void commands.rescanLibrary();
      }
    }

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, []);
}
