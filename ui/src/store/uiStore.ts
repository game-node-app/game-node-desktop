import { Store } from '@tanstack/react-store';
import { useStore } from '@tanstack/react-store';

export type SettingsTopic =
  | 'account'
  | 'stores'
  | 'drmFree'
  | 'backups'
  | 'general'
  | 'tools'
  | 'plugins'
  | 'about';

/** Pending values for one Settings panel that saves as a group (D4). */
export interface SettingsDraftValues {
  automatic?: boolean;
  destination?: string;
  retention?: number;
}

/**
 * One panel's unsaved edits. Only Backups saves as a group today; the shape is
 * generic so later group-saved panels add their own topic and values.
 */
export interface SettingsDraft {
  topic: 'backups';
  values: SettingsDraftValues;
}

/**
 * The content-area destination. It is deliberately not persisted: the application always
 * opens on the library, and only the library layout (view, sort, filter, selection) is
 * restored.
 */
export type ActiveDestination = 'library' | 'ports' | 'catalog';

export interface ContextMenuState {
  open: boolean;
  x: number;
  y: number;
  entryKey: string | null;
}

export type ToastKind = 'success' | 'danger' | 'info' | 'progress';

/** One notification action. The callback stays in the UI; it is never sent to the engine. */
export interface ToastAction {
  label: string;
  onClick: () => void;
}

/**
 * One notification, per D10. The container shows at most two actions; `progress` is a whole
 * percent and `persistent` keeps the notification until it is dismissed.
 */
export interface ToastItem {
  id: string;
  kind: ToastKind;
  title: string;
  body?: string;
  actions?: ToastAction[];
  progress?: number;
  persistent?: boolean;
}

/** What `pushNotification` accepts: the store assigns the id unless the caller supplies one. */
export type ToastNotification = Omit<ToastItem, 'id'> & { id?: string };

export interface UiStoreState {
  appMenuOpen: boolean;
  settingsOpen: boolean;
  settingsTopic: SettingsTopic;
  licenseModalOpen: boolean;
  activeDestination: ActiveDestination;
  /** Catalog entry the library's provenance link asked PC ports to show. */
  portsFocusCatalogId: string | null;
  sidebarDragWidth: number | null;
  detailDragWidth: number | null;
  contextMenu: ContextMenuState;
  localEditorEntryKey: string | null;
  saveProtectionEntryKey: string | null;
  discoveryModalOpen: boolean;
  removeModalEntryKey: string | null;
  /** Pending values of the Backups card, saved together through the unsaved bar. */
  settingsDraft: SettingsDraft | null;
  /** Save error shown on the changed destination row until the next edit. */
  settingsDraftError: string | null;
  discardBackupConfirmOpen: boolean;
  pendingSettingsAction: (() => void) | null;
  /** The command palette (Ctrl+K), a shared-modal-layer overlay. */
  paletteOpen: boolean;
  /** The keyboard shortcuts dialog, opened by `?`, the app menu or the palette. */
  shortcutsOpen: boolean;
  /** The save-view dialog. `renameViewId` selects its rename mode. */
  saveViewOpen: boolean;
  renameViewId: number | null;
  /** The filter popover, so Ctrl+Shift+F and the toolbar share one state. */
  filtersOpen: boolean;
  /** Session-only banner dismissals, keyed by `kind:provider:problemSince`. */
  dismissedBanners: Set<string>;
  toasts: ToastItem[];
}

/**
 * A fresh store state. Every reset builds new collections, so a reset never leaves a mutated
 * Set or array behind from the previous state.
 */
export function createInitialUiStoreState(): UiStoreState {
  return {
    appMenuOpen: false,
    settingsOpen: false,
    settingsTopic: 'account',
    licenseModalOpen: false,
    activeDestination: 'library',
    portsFocusCatalogId: null,
    sidebarDragWidth: null,
    detailDragWidth: null,
    contextMenu: {
      open: false,
      x: 0,
      y: 0,
      entryKey: null,
    },
    localEditorEntryKey: null,
    saveProtectionEntryKey: null,
    discoveryModalOpen: false,
    removeModalEntryKey: null,
    settingsDraft: null,
    settingsDraftError: null,
    discardBackupConfirmOpen: false,
    pendingSettingsAction: null,
    paletteOpen: false,
    shortcutsOpen: false,
    saveViewOpen: false,
    renameViewId: null,
    filtersOpen: false,
    dismissedBanners: new Set<string>(),
    toasts: [],
  };
}

export const initialUiStoreState: UiStoreState = createInitialUiStoreState();

export const uiStore = new Store<UiStoreState>(initialUiStoreState);

export function resetUiStore(): void {
  uiStore.setState(() => createInitialUiStoreState());
}

/**
 * Switches the content area. Selecting the library destination does not clear its filter;
 * the sidebar sends the explicit filter through `library_input` as well.
 */
export function setActiveDestination(destination: ActiveDestination): void {
  uiStore.setState((prev) =>
    prev.activeDestination === destination ? prev : { ...prev, activeDestination: destination },
  );
}

export function openSettingsModal(topic?: SettingsTopic): void {
  uiStore.setState((prev) => ({
    ...prev,
    appMenuOpen: false,
    settingsOpen: true,
    settingsTopic: topic ?? prev.settingsTopic,
  }));
}

export function closeSettingsModal(): void {
  uiStore.setState((prev) => ({
    ...prev,
    settingsOpen: false,
    licenseModalOpen: false,
    settingsDraft: null,
    settingsDraftError: null,
    discardBackupConfirmOpen: false,
    pendingSettingsAction: null,
  }));
}

export function setSettingsTopic(topic: SettingsTopic): void {
  uiStore.setState((prev) => ({
    ...prev,
    settingsTopic: topic,
  }));
}

export function openPalette(): void {
  uiStore.setState((prev) =>
    prev.paletteOpen ? prev : { ...prev, appMenuOpen: false, paletteOpen: true },
  );
}

export function closePalette(): void {
  uiStore.setState((prev) => (prev.paletteOpen ? { ...prev, paletteOpen: false } : prev));
}

export function openShortcuts(): void {
  uiStore.setState((prev) =>
    prev.shortcutsOpen ? prev : { ...prev, appMenuOpen: false, shortcutsOpen: true },
  );
}

export function closeShortcuts(): void {
  uiStore.setState((prev) => (prev.shortcutsOpen ? { ...prev, shortcutsOpen: false } : prev));
}

/** Opens the dialog in save mode. */
export function openSaveView(): void {
  uiStore.setState((prev) => ({
    ...prev,
    saveViewOpen: true,
    renameViewId: null,
  }));
}

/** Opens the dialog in rename mode for one saved view. */
export function openRenameView(id: number): void {
  uiStore.setState((prev) => ({
    ...prev,
    saveViewOpen: true,
    renameViewId: id,
  }));
}

export function closeSaveView(): void {
  uiStore.setState((prev) =>
    prev.saveViewOpen || prev.renameViewId != null
      ? { ...prev, saveViewOpen: false, renameViewId: null }
      : prev,
  );
}

export function setFiltersOpen(open: boolean): void {
  uiStore.setState((prev) => (prev.filtersOpen === open ? prev : { ...prev, filtersOpen: open }));
}

export function openFilters(): void {
  setFiltersOpen(true);
}

export function closeFilters(): void {
  setFiltersOpen(false);
}

/** Dismisses one banner for this session. The key changes when the problem recovers and returns. */
export function dismissBanner(key: string): void {
  uiStore.setState((prev) => {
    if (prev.dismissedBanners.has(key)) return prev;
    const dismissedBanners = new Set(prev.dismissedBanners);
    dismissedBanners.add(key);
    return { ...prev, dismissedBanners };
  });
}

export function clearDismissedBanners(): void {
  uiStore.setState((prev) =>
    prev.dismissedBanners.size === 0 ? prev : { ...prev, dismissedBanners: new Set<string>() },
  );
}

function newToastId(): string {
  return `toast-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
}

function normalizeActions(actions: ToastAction[] | undefined): ToastAction[] | undefined {
  if (!actions) return undefined;
  const kept = actions.slice(0, 2);
  return kept.length > 0 ? kept : undefined;
}

/**
 * Adds one D10 notification. A caller-supplied id replaces that notification instead of
 * duplicating it; otherwise the store generates one.
 */
export function pushNotification(notification: ToastNotification): string {
  const id = notification.id ?? newToastId();
  const item: ToastItem = {
    id,
    kind: notification.kind,
    title: notification.title,
    body: notification.body,
    actions: normalizeActions(notification.actions),
    progress: notification.progress,
    persistent: notification.persistent,
  };
  uiStore.setState((prev) => {
    const at = prev.toasts.findIndex((toast) => toast.id === id);
    if (at < 0) return { ...prev, toasts: [...prev.toasts, item] };
    const toasts = [...prev.toasts];
    toasts[at] = item;
    return { ...prev, toasts };
  });
  return id;
}

/** Patches one notification in place. An unknown id changes nothing. */
export function updateNotification(id: string, patch: Partial<Omit<ToastItem, 'id'>>): void {
  uiStore.setState((prev) => {
    const at = prev.toasts.findIndex((toast) => toast.id === id);
    if (at < 0) return prev;
    const toasts = [...prev.toasts];
    toasts[at] = {
      ...toasts[at],
      ...patch,
      actions: patch.actions === undefined ? toasts[at].actions : normalizeActions(patch.actions),
    };
    return { ...prev, toasts };
  });
}

/** Backward-compatible shorthand used by existing Settings callers. */
export function pushToast(kind: 'success' | 'danger', message: string): string {
  return pushNotification({ kind, title: message });
}

export function dismissToast(id: string): void {
  uiStore.setState((prev) =>
    prev.toasts.some((toast) => toast.id === id)
      ? { ...prev, toasts: prev.toasts.filter((toast) => toast.id !== id) }
      : prev,
  );
}

export function openContextMenu(x: number, y: number, entryKey: string): void {
  uiStore.setState((prev) => ({
    ...prev,
    contextMenu: {
      open: true,
      x,
      y,
      entryKey,
    },
  }));
}

export function closeContextMenu(): void {
  uiStore.setState((prev) => ({
    ...prev,
    contextMenu: {
      ...prev.contextMenu,
      open: false,
      entryKey: null,
    },
  }));
}

export function useUiStore<TSelected = UiStoreState>(
  selector?: (state: UiStoreState) => TSelected,
): TSelected {
  return useStore(uiStore, selector ?? ((s) => s as unknown as TSelected));
}
