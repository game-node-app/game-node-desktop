import React, { useEffect, useRef } from 'react';
import { IconAlertTriangle, IconX } from '@tabler/icons-react';
import { FloatingModal } from '../../components/FloatingModal';
import {
  closeSettingsModal,
  setSettingsTopic,
  uiStore,
  useUiStore,
  type SettingsTopic,
} from '../../store/uiStore';
import { useAppInfo, useAuth, useLocalGames, useSnapshot } from '../../api';
import { useI18n } from '../../i18n';

import { AccountPanel } from './panels/AccountPanel';
import { StoresPanel } from './panels/StoresPanel';
import { BackupsPanel } from './panels/BackupsPanel';
import { ToolsPanel } from './panels/ToolsPanel';
import { AboutPanel } from './panels/AboutPanel';
import { DiscardChangesDialog } from './DiscardChangesDialog';
import { LicenseNoticesModal } from './LicenseNoticesModal';
import { settingsCloseButton } from './settingsClasses';

interface TopicItem {
  id: SettingsTopic;
  titleKey: string;
  descKey: string;
}

const TOPICS: TopicItem[] = [
  {
    id: 'account',
    titleKey: 'Account',
    descKey: 'Your GameNode connection and what this PC syncs.',
  },
  {
    id: 'stores',
    titleKey: 'Stores',
    descKey: 'Stores found on this PC and how playtime is read.',
  },
  {
    id: 'backups',
    titleKey: 'Backups',
    descKey: 'Local protection for your DRM-free game saves.',
  },
  {
    id: 'tools',
    titleKey: 'Tools',
    descKey: 'Upscalers, frame generation tools, and local download cache.',
  },
  {
    id: 'about',
    titleKey: 'About',
    descKey: 'Version, license and third-party notices.',
  },
];

export const SettingsModal: React.FC = () => {
  const { t } = useI18n();
  const settingsOpen = useUiStore((s) => s.settingsOpen);
  const settingsTopic = useUiStore((s) => s.settingsTopic);
  const licenseModalOpen = useUiStore((s) => s.licenseModalOpen);
  const discardBackupConfirmOpen = useUiStore((s) => s.discardBackupConfirmOpen);
  const backupDestinationDraft = useUiStore((s) => s.backupDestinationDraft);

  const { auth } = useAuth();
  const { data: appInfo } = useAppInfo();
  const localGames = useLocalGames();
  const { data: storeAccounts } = useSnapshot((s) => s.storeAccounts);
  const storesNeedsAttention = (storeAccounts ?? []).some(
    (account) => account.state === 'expired' || account.state === 'credentialError' || account.persistFailed,
  );

  const contentPaneRef = useRef<HTMLDivElement>(null);
  const showLicenseNoticesButtonRef = useRef<HTMLButtonElement>(null);

  const currentDestination = localGames?.settings?.destination ?? '';
  const isDirty =
    backupDestinationDraft !== null && backupDestinationDraft !== currentDestination;

  // Reset scroll position when topic changes
  useEffect(() => {
    if (contentPaneRef.current) {
      contentPaneRef.current.scrollTop = 0;
    }
  }, [settingsTopic]);

  const handleRequestClose = () => {
    if (isDirty) {
      uiStore.setState((prev) => ({
        ...prev,
        discardBackupConfirmOpen: true,
        pendingSettingsAction: () => closeSettingsModal(),
      }));
    } else {
      closeSettingsModal();
    }
  };

  const handleSelectTopic = (targetTopic: SettingsTopic) => {
    if (targetTopic === settingsTopic) return;

    if (isDirty) {
      uiStore.setState((prev) => ({
        ...prev,
        discardBackupConfirmOpen: true,
        pendingSettingsAction: () => {
          uiStore.setState((p) => ({ ...p, backupDestinationDraft: null }));
          setSettingsTopic(targetTopic);
        },
      }));
    } else {
      setSettingsTopic(targetTopic);
      if (contentPaneRef.current) {
        contentPaneRef.current.scrollTop = 0;
      }
    }
  };

  const handleCancelDiscard = () => {
    uiStore.setState((prev) => ({
      ...prev,
      discardBackupConfirmOpen: false,
      pendingSettingsAction: null,
    }));
  };

  const handleConfirmDiscard = () => {
    const pendingAction = uiStore.state.pendingSettingsAction;
    uiStore.setState((prev) => ({
      ...prev,
      backupDestinationDraft: null,
      discardBackupConfirmOpen: false,
      pendingSettingsAction: null,
    }));
    if (pendingAction) {
      pendingAction();
    }
  };

  const handleCloseLicenseModal = () => {
    uiStore.setState((prev) => ({
      ...prev,
      licenseModalOpen: false,
    }));
    showLicenseNoticesButtonRef.current?.focus();
  };

  if (!settingsOpen) {
    return null;
  }

  const isSignedIn = typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const user = isSignedIn ? auth.signedIn.user : null;
  const identity = user?.username ?? user?.email ?? user?.id ?? '';
  const initial = identity ? identity[0].toUpperCase() : 'U';

  const activeTopic = TOPICS.find((t) => t.id === settingsTopic) ?? TOPICS[0];

  return (
    <>
      <FloatingModal.Root
        opened
        onClose={handleRequestClose}
        geometryKey="settings"
        defaultSize={{ width: 960, height: 720 }}
        minSize={{ width: 720, height: 480 }}
        zIndex={1000}
        closeOnEscape
        closeOnClickOutside
      >
        <FloatingModal.Overlay backgroundOpacity={0.62} />
        <FloatingModal.Content
          aria-label={t('settings.settings') || 'Settings'}
          className="flex flex-row bg-[var(--settings-bg-base)] p-0 text-white"
        >
          {/* Left Sidebar */}
          <aside className="flex w-sidebar shrink-0 flex-col gap-sm border-r border-border-subtle bg-chrome px-sm py-md">
            {/* User Block */}
            <div
              data-floating-modal-handle=""
              className="flex cursor-grab touch-none items-center gap-[10px] rounded-sm bg-secondary p-compact select-none active:cursor-grabbing"
            >
              <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-brand text-sm font-semibold text-white">
                {isSignedIn ? initial : '•'}
              </div>
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-sm font-semibold text-white">
                  {isSignedIn ? identity : t('settings.noSession') || 'No session'}
                </span>
                <span className="text-xs text-[var(--settings-text-secondary)]">
                  {isSignedIn ? t('settings.signedIn') || 'Signed in' : t('settings.noSession') || 'No session'}
                </span>
              </div>
            </div>

            <div className="h-px bg-border-subtle" />

            <span className="px-compact py-1 text-xs font-bold uppercase tracking-[0.05em] text-[var(--settings-text-muted)]">
              {t('settings.settings') || 'SETTINGS'}
            </span>

            {/* Topics Navigation */}
            <nav className="flex flex-col gap-1" aria-label="Settings Topics">
              {TOPICS.map((topic) => {
                const isSelected = topic.id === settingsTopic;
                const needsAttention = topic.id === 'stores' && storesNeedsAttention;
                return (
                  <button
                    key={topic.id}
                    type="button"
                    aria-current={isSelected ? 'page' : undefined}
                    aria-label={
                      needsAttention
                        ? `${t(topic.titleKey) || topic.titleKey}: ${t('settings.storesNeedsAttention')}`
                        : undefined
                    }
                    className={`flex w-full items-center gap-[10px] rounded-sm px-sm py-compact text-left text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand ${
                      isSelected
                        ? 'active bg-accent-subtle font-semibold text-brand'
                        : 'font-medium text-[var(--settings-text-secondary)] hover:bg-raised hover:text-white'
                    }`}
                    onClick={() => handleSelectTopic(topic.id)}
                  >
                    <span className="min-w-0 flex-1">{t(topic.titleKey) || topic.titleKey}</span>
                    {needsAttention && (
                      <span
                        role="img"
                        aria-label={t('settings.storesNeedsAttention')}
                        className="inline-flex shrink-0 items-center gap-[4px] rounded-full bg-warning-subtle px-compact py-[2px] text-[10px] font-semibold uppercase tracking-[0.04em] text-warning"
                      >
                        <IconAlertTriangle size={13} stroke={1.75} aria-hidden="true" />
                        <span>{t('settings.attention')}</span>
                      </span>
                    )}
                  </button>
                );
              })}
            </nav>

            {/* Sidebar Footer */}
            <div className="mt-auto truncate p-compact text-xs text-[var(--settings-text-muted)]">
              GameNode · {appInfo?.version ?? '0.1.0'}
            </div>
          </aside>

          {/* Right Content Pane */}
          <section className="flex min-w-0 flex-1 flex-col bg-[var(--settings-bg-base)]">
            {/* Header */}
            <header
              data-floating-modal-handle=""
              className="flex cursor-grab touch-none items-start justify-between border-b border-border-subtle px-control-inline py-lg select-none active:cursor-grabbing"
            >
              <div className="flex flex-col gap-1">
                <h2 className="text-lg font-bold text-white">
                  {t(activeTopic.titleKey) || activeTopic.titleKey}
                </h2>
                <span className="text-sm text-[var(--settings-text-secondary)]">
                  {t(activeTopic.descKey) || activeTopic.descKey}
                </span>
              </div>
              <button
                type="button"
                className={settingsCloseButton}
                onClick={handleRequestClose}
                aria-label={t('settings.closeSettings') || 'Close settings'}
              >
                <IconX size={18} stroke={1.75} aria-hidden="true" />
              </button>
            </header>

            {/* Scrollable Content */}
            <div
              ref={contentPaneRef}
              className="settings-scroll-content flex flex-1 flex-col gap-6 overflow-y-auto p-6"
            >
              {settingsTopic === 'account' && <AccountPanel />}
              {settingsTopic === 'stores' && <StoresPanel />}
              {settingsTopic === 'backups' && <BackupsPanel />}
              {settingsTopic === 'tools' && <ToolsPanel />}
              {settingsTopic === 'about' && (
                <AboutPanel showLicenseNoticesButtonRef={showLicenseNoticesButtonRef} />
              )}
            </div>
          </section>

          <FloatingModal.ResizeHandle />
        </FloatingModal.Content>
      </FloatingModal.Root>

      {/* Overlays on top of settings modal */}
      {discardBackupConfirmOpen && (
        <DiscardChangesDialog
          onCancel={handleCancelDiscard}
          onConfirm={handleConfirmDiscard}
        />
      )}

      {licenseModalOpen && (
        <LicenseNoticesModal onClose={handleCloseLicenseModal} />
      )}
    </>
  );
};
