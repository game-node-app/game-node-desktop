import { Store } from '@tanstack/react-store';
import { useStore } from '@tanstack/react-store';

export type SettingsTopic = 'account' | 'stores' | 'backups' | 'tools' | 'plugins' | 'about';

export interface ContextMenuState {
  open: boolean;
  x: number;
  y: number;
  entryKey: string | null;
}

export interface ToastItem {
  id: string;
  kind: 'success' | 'danger';
  message: string;
}

export interface UiStoreState {
  appMenuOpen: boolean;
  settingsOpen: boolean;
  settingsTopic: SettingsTopic;
  licenseModalOpen: boolean;
  sidebarDragWidth: number | null;
  detailDragWidth: number | null;
  contextMenu: ContextMenuState;
  localEditorEntryKey: string | null;
  saveProtectionEntryKey: string | null;
  discoveryModalOpen: boolean;
  removeModalEntryKey: string | null;
  backupDestinationDraft: string | null;
  discardBackupConfirmOpen: boolean;
  pendingSettingsAction: (() => void) | null;
  toasts: ToastItem[];
}

export const initialUiStoreState: UiStoreState = {
  appMenuOpen: false,
  settingsOpen: false,
  settingsTopic: 'account',
  licenseModalOpen: false,
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
  backupDestinationDraft: null,
  discardBackupConfirmOpen: false,
  pendingSettingsAction: null,
  toasts: [],
};

export const uiStore = new Store<UiStoreState>(initialUiStoreState);

export function resetUiStore(): void {
  uiStore.setState(() => ({ ...initialUiStoreState }));
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
    backupDestinationDraft: null,
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

export function pushToast(kind: 'success' | 'danger', message: string): string {
  const id = `toast-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
  uiStore.setState((prev) => ({
    ...prev,
    toasts: [...prev.toasts, { id, kind, message }],
  }));
  return id;
}

export function dismissToast(id: string): void {
  uiStore.setState((prev) => ({
    ...prev,
    toasts: prev.toasts.filter((t) => t.id !== id),
  }));
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
