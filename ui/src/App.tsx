import React, { useEffect, useRef } from 'react';
import {
  useAuth,
  useBootstrapState,
  useEngineSnapshotSubscription,
  useLibraryView,
} from './api';
import { useI18n } from './i18n';
import { SignedOutView } from './features/auth';
import { AddDialog, MatchDialog } from './features/dialogs';
import {
  ClaimConflictModal,
  DiscoveryModal,
  LocalGameEditor,
  RemoveLocalGameModal,
  SaveProtectionModal,
  useLocalGameErrorToast,
} from './features/drm-free';
import { LibraryScreen } from './features/library';
import {
  SettingsModal,
  ToastContainer,
  useSettingsKeyboardShortcut,
} from './features/settings';
import { Sidebar, SyncExplanationModal, TopBar } from './features/shell';
import { GameToolsPanel } from './features/tools';

/** Shown between sign-in completing and the engine opening the account's library. */
const ConnectedPlaceholder: React.FC = () => {
  const { t } = useI18n();
  return (
    <div
      data-testid="library-loading"
      style={{
        flex: 1,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: '8px',
        color: 'var(--color-text-dimmed)',
      }}
    >
      <h2 style={{ color: 'var(--color-text-default)', fontSize: 'var(--font-size-xl)' }}>
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
  const bootstrap = useBootstrapState();

  // Bridge `engine:snapshot` events into the TanStack Query cache.
  useEngineSnapshotSubscription();
  useSettingsKeyboardShortcut();
  useLocalGameErrorToast();

  // Adopt the OS locale the host reports once, so a manual switch is not overwritten later.
  const localeApplied = useRef(false);
  const hostLocale = bootstrap.data?.locale;
  useEffect(() => {
    if (hostLocale && !localeApplied.current) {
      localeApplied.current = true;
      setLocale(hostLocale);
    }
  }, [hostLocale, setLocale]);

  const signedIn = typeof auth === 'object' && 'signedIn' in auth;

  let main: React.ReactNode;
  if (library) {
    main = <LibraryScreen />;
  } else if (signedIn) {
    main = <ConnectedPlaceholder />;
  } else {
    main = <SignedOutView />;
  }

  return (
    <div
      data-testid="app-root"
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        overflow: 'hidden',
        backgroundColor: 'var(--color-background)',
      }}
    >
      <TopBar />
      <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
        <Sidebar />
        <main
          style={{
            flex: 1,
            minWidth: 0,
            display: 'flex',
            flexDirection: 'column',
            overflow: 'auto',
          }}
        >
          {main}
        </main>
      </div>

      {/* Overlays: each one gates itself on the snapshot or the UI store. */}
      <SyncExplanationModal />
      <SettingsModal />
      <MatchDialog />
      <AddDialog />
      <LocalGameEditor />
      <DiscoveryModal />
      <ClaimConflictModal />
      <RemoveLocalGameModal />
      <SaveProtectionModal />
      <GameToolsPanel />
      <ToastContainer />
    </div>
  );
};
