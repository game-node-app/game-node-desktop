import React, { useEffect, useRef } from 'react';
import { useAppInfo, useAuth, useLibraryView } from './api';
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

  useSettingsKeyboardShortcut();
  useLocalGameErrorToast();

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
      className="flex h-full w-full flex-col overflow-hidden bg-body"
    >
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <Sidebar />
        <main className="flex min-w-0 flex-1 flex-col overflow-auto">
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
