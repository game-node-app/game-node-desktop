import type { LibraryView } from '../../api';
import { uiStore } from '../../store/uiStore';

/**
 * Keyboard targets that type characters. Shortcut listeners must ignore them
 * so typing "f" or "s" in a search box never changes the library.
 */
export function isTextEntryTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return true;
  if (target.isContentEditable) return true;
  return target.closest('[contenteditable="true"]') !== null;
}

/**
 * Overlays that own the keyboard. The components in this unit install their
 * own library shortcuts (Ctrl+F, Ctrl+Shift+F, Shift+S); they must not fire
 * while a dialog, menu or palette is open. The later `useGlobalShortcuts` hook
 * reads the same helper, so the two never double-handle a key.
 */
export function isShortcutBlockedByOverlay(
  library?: LibraryView | null,
  options?: { allowFiltersOpen?: boolean },
): boolean {
  const state = uiStore.state;
  if (
    state.appMenuOpen ||
    state.settingsOpen ||
    state.licenseModalOpen ||
    state.paletteOpen ||
    state.shortcutsOpen ||
    state.saveViewOpen ||
    state.contextMenu.open ||
    state.discoveryModalOpen ||
    state.localEditorEntryKey !== null ||
    state.saveProtectionEntryKey !== null ||
    state.removeModalEntryKey !== null ||
    state.backupDestinationDraft !== null ||
    state.discardBackupConfirmOpen ||
    (!options?.allowFiltersOpen && state.filtersOpen)
  ) {
    return true;
  }
  // Menus (sort, group, app, context) are overlays too, even though their
  // open state is local to each Mantine Menu rather than `uiStore`.
  if (typeof document !== 'undefined' && document.querySelector('[role="menu"]')) {
    return true;
  }
  return Boolean(library?.addDialog || library?.matchDialog);
}

/**
 * Shared guard for the library-control shortcuts. It ignores text fields,
 * events another handler already consumed, and any open overlay.
 */
export function shouldIgnoreLibraryShortcut(
  event: KeyboardEvent,
  library?: LibraryView | null,
  options?: { allowFiltersOpen?: boolean },
): boolean {
  return (
    event.defaultPrevented ||
    isTextEntryTarget(event.target) ||
    isShortcutBlockedByOverlay(library, options)
  );
}
