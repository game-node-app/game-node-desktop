import React, { useEffect, useRef } from 'react';
import {
  closeSettingsModal,
  setSettingsTopic,
  uiStore,
  useUiStore,
  type SettingsTopic,
} from '../../store/uiStore';
import { useAuth, useBootstrapState, useLocalGames } from '../../api';
import { useI18n } from '../../i18n';
import './settings.css';

import { AccountPanel } from './panels/AccountPanel';
import { StoresPanel } from './panels/StoresPanel';
import { BackupsPanel } from './panels/BackupsPanel';
import { ToolsPanel } from './panels/ToolsPanel';
import { AboutPanel } from './panels/AboutPanel';
import { DiscardChangesDialog } from './DiscardChangesDialog';
import { LicenseNoticesModal } from './LicenseNoticesModal';

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
  const { data: bootstrap } = useBootstrapState();
  const localGames = useLocalGames();

  const contentPaneRef = useRef<HTMLDivElement>(null);
  const modalRef = useRef<HTMLDivElement>(null);
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

  // Global escape key handler when settings modal is open
  useEffect(() => {
    if (!settingsOpen) return;

    function handleWindowKeyDown(event: KeyboardEvent) {
      if (event.key === 'Escape') {
        const state = uiStore.state;
        if (state.licenseModalOpen || state.discardBackupConfirmOpen) {
          return;
        }

        event.preventDefault();
        const dirty =
          state.backupDestinationDraft !== null &&
          state.backupDestinationDraft !== (localGames?.settings?.destination ?? '');

        if (dirty) {
          uiStore.setState((prev) => ({
            ...prev,
            discardBackupConfirmOpen: true,
            pendingSettingsAction: () => closeSettingsModal(),
          }));
        } else {
          closeSettingsModal();
        }
      }
    }

    window.addEventListener('keydown', handleWindowKeyDown);
    return () => {
      window.removeEventListener('keydown', handleWindowKeyDown);
    };
  }, [settingsOpen, localGames?.settings?.destination]);

  // Tab key focus trapping inside active dialog
  const handleModalKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (licenseModalOpen || discardBackupConfirmOpen) return;

    if (event.key === 'Tab' && modalRef.current) {
      const focusables = modalRef.current.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])'
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];

      if (event.shiftKey) {
        if (document.activeElement === first || !modalRef.current.contains(document.activeElement)) {
          event.preventDefault();
          last.focus();
        }
      } else {
        if (document.activeElement === last || !modalRef.current.contains(document.activeElement)) {
          event.preventDefault();
          first.focus();
        }
      }
    }
  };

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
    <div
      className="settings-overlay"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          handleRequestClose();
        }
      }}
    >
      <div
        ref={modalRef}
        className="settings-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t('settings.settings') || 'Settings'}
        onKeyDown={handleModalKeyDown}
      >
        {/* Left Sidebar */}
        <aside className="settings-sidebar">
          {/* User Block */}
          <div className="settings-user-block">
            <div className="settings-user-avatar">{isSignedIn ? initial : '•'}</div>
            <div className="settings-user-info">
              <span className="settings-user-name">
                {isSignedIn ? identity : t('settings.noSession') || 'No session'}
              </span>
              <span className="settings-user-status">
                {isSignedIn ? t('settings.signedIn') || 'Signed in' : t('settings.noSession') || 'No session'}
              </span>
            </div>
          </div>

          <div
            style={{
              height: '1px',
              backgroundColor: 'var(--settings-border-subtle, #2a2a2a)',
            }}
          />

          <span className="settings-group-header">
            {t('settings.settings') || 'SETTINGS'}
          </span>

          {/* Topics Navigation */}
          <nav className="settings-nav" aria-label="Settings Topics">
            {TOPICS.map((topic) => {
              const isSelected = topic.id === settingsTopic;
              return (
                <button
                  key={topic.id}
                  type="button"
                  className={`settings-nav-item ${isSelected ? 'active' : ''}`}
                  onClick={() => handleSelectTopic(topic.id)}
                >
                  {t(topic.titleKey) || topic.titleKey}
                </button>
              );
            })}
          </nav>

          {/* Sidebar Footer */}
          <div className="settings-sidebar-footer">
            GameNode · {bootstrap?.version ?? '0.1.0'}
          </div>
        </aside>

        {/* Right Content Pane */}
        <section className="settings-content-wrapper">
          {/* Header */}
          <header className="settings-header">
            <div className="settings-header-titles">
              <h2 className="settings-header-title">{t(activeTopic.titleKey) || activeTopic.titleKey}</h2>
              <span className="settings-header-description">{t(activeTopic.descKey) || activeTopic.descKey}</span>
            </div>
            <button
              type="button"
              className="settings-close-button"
              onClick={handleRequestClose}
              aria-label={t('settings.closeSettings') || 'Close settings'}
            >
              ×
            </button>
          </header>

          {/* Scrollable Content */}
          <div ref={contentPaneRef} className="settings-scroll-content">
            {settingsTopic === 'account' && <AccountPanel />}
            {settingsTopic === 'stores' && <StoresPanel />}
            {settingsTopic === 'backups' && <BackupsPanel />}
            {settingsTopic === 'tools' && <ToolsPanel />}
            {settingsTopic === 'about' && (
              <AboutPanel showLicenseNoticesButtonRef={showLicenseNoticesButtonRef} />
            )}
          </div>
        </section>

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
      </div>
    </div>
  );
};
