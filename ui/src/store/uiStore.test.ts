import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  clearDismissedBanners,
  closeFilters,
  closePalette,
  closeSaveView,
  closeSettingsModal,
  closeShortcuts,
  createInitialUiStoreState,
  dismissBanner,
  dismissToast,
  initialUiStoreState,
  openFilters,
  openPalette,
  openRenameView,
  openSaveView,
  openShortcuts,
  pushNotification,
  pushToast,
  resetUiStore,
  setActiveDestination,
  uiStore,
  updateNotification,
} from './uiStore';

describe('uiStore destinations and overlays', () => {
  beforeEach(() => {
    resetUiStore();
  });

  it('opens the catalog as a content destination while keeping the library state intact', () => {
    expect(uiStore.state.activeDestination).toBe('library');

    setActiveDestination('catalog');
    expect(uiStore.state.activeDestination).toBe('catalog');

    setActiveDestination('ports');
    expect(uiStore.state.activeDestination).toBe('ports');

    setActiveDestination('library');
    expect(uiStore.state.activeDestination).toBe('library');
  });

  it('owns the palette and shortcuts dialog state', () => {
    openPalette();
    expect(uiStore.state.paletteOpen).toBe(true);
    expect(uiStore.state.appMenuOpen).toBe(false);

    closePalette();
    expect(uiStore.state.paletteOpen).toBe(false);

    openShortcuts();
    expect(uiStore.state.shortcutsOpen).toBe(true);
    closeShortcuts();
    expect(uiStore.state.shortcutsOpen).toBe(false);
  });

  it('opens the save-view dialog in save and rename modes', () => {
    openSaveView();
    expect(uiStore.state.saveViewOpen).toBe(true);
    expect(uiStore.state.renameViewId).toBeNull();

    closeSaveView();
    expect(uiStore.state.saveViewOpen).toBe(false);

    openRenameView(42);
    expect(uiStore.state.saveViewOpen).toBe(true);
    expect(uiStore.state.renameViewId).toBe(42);

    closeSaveView();
    expect(uiStore.state.saveViewOpen).toBe(false);
    expect(uiStore.state.renameViewId).toBeNull();
  });

  it('opens and closes the filters popover', () => {
    openFilters();
    expect(uiStore.state.filtersOpen).toBe(true);

    closeFilters();
    expect(uiStore.state.filtersOpen).toBe(false);
  });

  it('dismisses banners with a new Set and clears them without mutating the old one', () => {
    const before = uiStore.state.dismissedBanners;

    dismissBanner('offline:gamenode:1');
    const dismissed = uiStore.state.dismissedBanners;
    expect(dismissed).not.toBe(before);
    expect(dismissed.has('offline:gamenode:1')).toBe(true);
    expect(before.has('offline:gamenode:1')).toBe(false);

    dismissBanner('expired:steam:2');
    dismissBanner('expired:steam:2');
    expect(uiStore.state.dismissedBanners.size).toBe(2);

    clearDismissedBanners();
    expect(uiStore.state.dismissedBanners.size).toBe(0);
    expect(uiStore.state.dismissedBanners).not.toBe(dismissed);
  });
});

describe('uiStore notifications', () => {
  beforeEach(() => {
    resetUiStore();
  });

  it('keeps pushToast as the short success/danger form', () => {
    const id = pushToast('success', 'Saved');

    expect(uiStore.state.toasts).toHaveLength(1);
    expect(uiStore.state.toasts[0]).toMatchObject({
      id,
      kind: 'success',
      title: 'Saved',
    });
    expect(uiStore.state.toasts[0].body).toBeUndefined();
  });

  it('adds a full notification and keeps action callbacks in the UI', () => {
    const undo = vi.fn();
    const view = vi.fn();
    const extra = vi.fn();
    const id = pushNotification({
      kind: 'progress',
      title: 'Downloading Port',
      body: '2 of 3 files',
      actions: [
        { label: 'Undo', onClick: undo },
        { label: 'View on GameNode', onClick: view },
        { label: 'Third', onClick: extra },
      ],
      progress: 42,
      persistent: true,
    });

    const toast = uiStore.state.toasts[0];
    expect(toast.id).toBe(id);
    expect(toast.kind).toBe('progress');
    expect(toast.title).toBe('Downloading Port');
    expect(toast.body).toBe('2 of 3 files');
    expect(toast.progress).toBe(42);
    expect(toast.persistent).toBe(true);
    expect(toast.actions).toHaveLength(2);
    expect(toast.actions?.[0].onClick).toBe(undo);
    expect(toast.actions?.[1].onClick).toBe(view);
    expect(toast.actions?.[0].onClick).not.toBe(extra);

    toast.actions?.[0].onClick();
    expect(undo).toHaveBeenCalledTimes(1);
  });

  it('replaces a notification with the same caller-supplied id', () => {
    pushNotification({ id: 'run-7', kind: 'progress', title: 'Running' });
    pushNotification({ id: 'run-7', kind: 'progress', title: 'Running', progress: 80 });

    expect(uiStore.state.toasts).toHaveLength(1);
    expect(uiStore.state.toasts[0]).toMatchObject({ id: 'run-7', progress: 80 });
  });

  it('patches a notification in place and leaves an unknown id untouched', () => {
    const id = pushNotification({
      kind: 'progress',
      title: 'Downloading',
      body: 'Starting',
      progress: 0,
    });

    updateNotification(id, {
      title: 'Downloaded',
      body: 'Ready to install',
      progress: 100,
      persistent: true,
    });

    expect(uiStore.state.toasts).toHaveLength(1);
    expect(uiStore.state.toasts[0]).toMatchObject({
      id,
      kind: 'progress',
      title: 'Downloaded',
      body: 'Ready to install',
      progress: 100,
      persistent: true,
    });

    const before = uiStore.state;
    updateNotification('missing', { title: 'Nothing' });
    expect(uiStore.state).toBe(before);
    expect(uiStore.state.toasts).toEqual(before.toasts);
  });

  it('updates the action list and still caps it at two', () => {
    const id = pushNotification({ kind: 'success', title: 'Added', actions: [] });

    updateNotification(id, {
      actions: [
        { label: 'Undo', onClick: vi.fn() },
        { label: 'View', onClick: vi.fn() },
        { label: 'Extra', onClick: vi.fn() },
      ],
    });

    expect(uiStore.state.toasts[0].actions).toHaveLength(2);
    updateNotification(id, { actions: [] });
    expect(uiStore.state.toasts[0].actions).toBeUndefined();
  });

  it('dismisses one toast by id', () => {
    const first = pushToast('success', 'First');
    pushToast('danger', 'Second');

    dismissToast(first);

    expect(uiStore.state.toasts).toHaveLength(1);
    expect(uiStore.state.toasts[0].title).toBe('Second');
  });
});

describe('uiStore reset', () => {
  it('clears the settings draft and its error when the modal closes', () => {
    uiStore.setState((prev) => ({
      ...prev,
      settingsOpen: true,
      settingsDraft: {
        topic: 'backups',
        values: { destination: '/unsaved' },
      },
      settingsDraftError: 'Invalid destination',
      discardBackupConfirmOpen: true,
    }));

    closeSettingsModal();

    expect(uiStore.state.settingsOpen).toBe(false);
    expect(uiStore.state.settingsDraft).toBeNull();
    expect(uiStore.state.settingsDraftError).toBeNull();
    expect(uiStore.state.discardBackupConfirmOpen).toBe(false);
  });

  it('builds new collections instead of reusing mutated initial state', () => {
    const initial = createInitialUiStoreState();
    expect(initial.dismissedBanners).toBeInstanceOf(Set);
    expect(initial.toasts).toEqual([]);

    dismissBanner('offline:gamenode:1');
    pushToast('success', 'Saved');
    setActiveDestination('catalog');
    openPalette();
    openFilters();

    resetUiStore();

    expect(uiStore.state.dismissedBanners.size).toBe(0);
    expect(uiStore.state.dismissedBanners).not.toBe(initial.dismissedBanners);
    expect(uiStore.state.toasts).toEqual([]);
    expect(uiStore.state.toasts).not.toBe(initial.toasts);
    expect(uiStore.state.activeDestination).toBe('library');
    expect(uiStore.state.paletteOpen).toBe(false);
    expect(uiStore.state.filtersOpen).toBe(false);
    expect(initialUiStoreState.dismissedBanners.size).toBe(0);
  });
});
