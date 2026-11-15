import React, { lazy, Suspense, useEffect, useRef } from 'react';
import { useAppInfo, useAuth, useLibraryView } from './api';
import { detectPortPlatform } from './api/ports';
import { useI18n } from './i18n';
import { SignedOutView } from './features/auth';
import { CatalogView } from './features/catalog/CatalogView';
import { LibraryScreen } from './features/library';
import { useLibraryNotifications } from './features/library/useLibraryNotifications';
import { PortsView } from './features/ports';
import { ToastContainer } from './features/settings/ToastContainer';
import { Sidebar, SyncExplanationModal, TopBar } from './features/shell';
import { CommandPalette } from './features/shell/CommandPalette';
import { ShortcutsModal } from './features/shell/ShortcutsModal';
import { StatusBar } from './features/shell/StatusBar';
import { useGlobalShortcuts } from './features/shell/useGlobalShortcuts';
import { useLocalGameErrorToast } from './features/drm-free/useLocalGameErrorToast';
import { usePluginNotices } from './features/plugins/usePluginNotices';
import { PluginPortSetupHost } from './features/ports/setup/PluginPortSetupHost';
import { PluginConfirmationHost } from './features/plugins/PluginConfirmationHost';
import { useUiStore } from './store/uiStore';

// Snapshot- or store-gated overlays are split out of the startup bundle. They are
// mounted eagerly (each renders null while closed), so the chunks load in the
// background and opening one never waits on the import.
const SettingsModal = lazy(() =>
  import('./features/settings/SettingsModal').then((m) => ({ default: m.SettingsModal })),
);
const MatchDialog = lazy(() =>
  import('./features/dialogs/MatchDialog').then((m) => ({ default: m.MatchDialog })),
);
const AddDialog = lazy(() =>
  import('./features/dialogs/AddDialog').then((m) => ({ default: m.AddDialog })),
);
const LocalGameEditor = lazy(() =>
  import('./features/drm-free/LocalGameEditor').then((m) => ({ default: m.LocalGameEditor })),
);
const DiscoveryModal = lazy(() =>
  import('./features/drm-free/DiscoveryModal').then((m) => ({ default: m.DiscoveryModal })),
);
const ClaimConflictModal = lazy(() =>
  import('./features/drm-free/ClaimConflictModal').then((m) => ({ default: m.ClaimConflictModal })),
);
const RemoveLocalGameModal = lazy(() =>
  import('./features/drm-free/RemoveLocalGameModal').then((m) => ({
    default: m.RemoveLocalGameModal,
  })),
);
const SaveProtectionModal = lazy(() =>
  import('./features/drm-free/SaveProtectionModal').then((m) => ({
    default: m.SaveProtectionModal,
  })),
);
const GameToolsPanel = lazy(() =>
  import('./features/tools/GameToolsPanel').then((m) => ({ default: m.GameToolsPanel })),
);

/**
 * A null fallback keeps the layout intact: every overlay is fixed-position or a
 * portal and renders nothing until it is opened.
 */
const LazyOverlay: React.FC<{ children: React.ReactNode }> = ({ children }) => (
  <Suspense fallback={null}>{children}</Suspense>
);

/** Shown between sign-in completing and the engine opening the account's library. */
const ConnectedPlaceholder: React.FC = () => {
  const { t } = useI18n();
  return (
    <div
      data-testid="library-loading"
      className="flex flex-1 flex-col items-center justify-center gap-2 text-text-dimmed"
    >
      <h2 className="text-xl text-text-default">
        {t('Your library is empty.')}
      </h2>
      <p>{t("You're all set. Your GameNode account is connected.")}</p>
    </div>
  );
};

export const App: React.FC = () => {
  const { setLocale } = useI18n();
  const { auth } = useAuth();
  const library = useLibraryView();
  const appInfo = useAppInfo();

  useGlobalShortcuts();
  useLibraryNotifications();
  useLocalGameErrorToast();
  usePluginNotices();

  // Adopt the OS locale the host reports once, so a manual switch is not overwritten later.
  const localeApplied = useRef(false);
  const hostLocale = appInfo.data?.locale;
  useEffect(() => {
    if (hostLocale && !localeApplied.current) {
      localeApplied.current = true;
      setLocale(hostLocale);
    }
  }, [hostLocale, setLocale]);

  const signedIn = typeof auth === 'object' && 'signedIn' in auth;

  // The destination is UI state, so it survives unrelated snapshot updates but never a
  // restart. Ports falls back to the library when the build is not an eligible Windows
  // signed-in build (for example after signing out).
  const destination = useUiStore((s) => s.activeDestination);
  const platform = detectPortPlatform(
    typeof navigator !== 'undefined' ? navigator.userAgent : null,
    appInfo.data?.platform,
  );
  const portsVisible = signedIn && platform === 'windows';

  let main: React.ReactNode;
  if (destination === 'ports' && portsVisible) {
    main = <PortsView />;
  } else if (destination === 'catalog') {
    main = <CatalogView />;
  } else if (library) {
    main = <LibraryScreen />;
  } else if (signedIn) {
    main = <ConnectedPlaceholder />;
  } else {
    main = <SignedOutView />;
  }

  return (
    <div
      data-testid="app-root"
      className="flex h-full w-full flex-col overflow-hidden bg-body"
    >
      <div className="grid min-h-0 flex-1 grid-rows-[48px_minmax(0,1fr)_26px]">
        <TopBar />
        <div className="flex min-h-0 min-w-0">
          <Sidebar />
          <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
            {main}
          </main>
        </div>
        <StatusBar />
      </div>

      {/* Overlays: each one gates itself on the snapshot or the UI store. */}
      <CommandPalette />
      <ShortcutsModal />
      <SyncExplanationModal />
      <LazyOverlay>
        <SettingsModal />
      </LazyOverlay>
      <LazyOverlay>
        <MatchDialog />
      </LazyOverlay>
      <LazyOverlay>
        <AddDialog />
      </LazyOverlay>
      <LazyOverlay>
        <LocalGameEditor />
      </LazyOverlay>
      <LazyOverlay>
        <DiscoveryModal />
      </LazyOverlay>
      <LazyOverlay>
        <ClaimConflictModal />
      </LazyOverlay>
      <LazyOverlay>
        <RemoveLocalGameModal />
      </LazyOverlay>
      <LazyOverlay>
        <SaveProtectionModal />
      </LazyOverlay>
      <LazyOverlay>
        <GameToolsPanel />
      </LazyOverlay>
      <PluginConfirmationHost />
      <PluginPortSetupHost />
      <ToastContainer />
    </div>
  );
};
