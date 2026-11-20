import React, {
  useCallback,
  useEffect,
  useRef,
  useState,
} from 'react';
import { useQuery } from '@tanstack/react-query';
import { IconSettings } from '@tabler/icons-react';
import { FloatingModal } from '../../components/FloatingModal';
import { Kbd } from '../../components/ui/Kbd';
import { NavItem } from '../../components/ui/NavItem';
import { cx } from '../../components/ui/cx';
import {
  closeSettingsModal,
  pushToast,
  setSettingsTopic,
  uiStore,
  useUiStore,
  type SettingsTopic,
} from '../../store/uiStore';
import {
  commands,
  useAppInfo,
  useLibraryView,
  useLocalGames,
  useSnapshot,
} from '../../api';
import { useI18n } from '../../i18n';
import { providerLabel } from '../../i18n/providerLabel';

import { AccountPanel } from './panels/AccountPanel';
import { StoresPanel } from './panels/StoresPanel';
import { DrmFreePanel } from './panels/DrmFreePanel';
import { BackupsPanel } from './panels/BackupsPanel';
import { GeneralPanel } from './panels/GeneralPanel';
import { ToolsPanel } from './panels/ToolsPanel';
import {
  PluginsPanel,
  PLUGIN_INVENTORY_QUERY_KEY,
} from './panels/PluginsPanel';
import { AboutPanel } from './panels/AboutPanel';
import { DiscardChangesDialog } from './DiscardChangesDialog';
import { LicenseNoticesModal } from './LicenseNoticesModal';
import {
  ScrollFade,
  SettingsSearch,
  SettingsSearchProvider,
  UnsavedChangesBar,
} from './components';
import { normalizeSettingsSearch } from './search';
import {
  BACKUP_DRAFT_FIELD_LABEL_KEYS,
  backupDraftChangedFields,
  backupDraftValue,
} from './settingsDraft';
import {
  settingsNavGroupsForSearch,
  settingsTopicsForSearch,
} from './topics';

/**
 * The Settings shell (design D3): 48px title bar used as the drag handle,
 * 232px grouped navigation with search, a scroller whose panel owns its title,
 * the bottom scroll fade and the pinned unsaved-changes bar. Geometry, overlay,
 * Escape/focus handling and the modal layer stay on the shared primitives.
 */
export const SettingsModal: React.FC = () => {
  const { t } = useI18n();
  const settingsOpen = useUiStore((state) => state.settingsOpen);
  const settingsTopic = useUiStore((state) => state.settingsTopic);
  const licenseModalOpen = useUiStore((state) => state.licenseModalOpen);
  const discardConfirmOpen = useUiStore(
    (state) => state.discardBackupConfirmOpen,
  );
  const settingsDraft = useUiStore((state) => state.settingsDraft);

  const { data: appInfo } = useAppInfo();
  const localGames = useLocalGames();
  const library = useLibraryView();
  const { data: storeAccounts } = useSnapshot((state) => state.storeAccounts);
  const { data: plugins } = useQuery({
    queryKey: PLUGIN_INVENTORY_QUERY_KEY,
    queryFn: commands.pluginInventory,
    staleTime: 30_000,
    enabled: settingsOpen,
  });

  const attentionAccount = (storeAccounts ?? []).find(
    (account) =>
      account.state === 'expired' ||
      account.state === 'credentialError' ||
      account.persistFailed,
  );
  const attentionProvider = attentionAccount
    ? providerLabel(
        attentionAccount.provider,
        library?.providers.find(
          (provider) => provider.id === attentionAccount.provider,
        )?.name,
        t,
      )
    : '';
  const storesAttentionLabel = attentionAccount
    ? t('settings.storeNeedsAttention', { provider: attentionProvider })
    : undefined;

  const [searchQuery, setSearchQuery] = useState('');
  const [saving, setSaving] = useState(false);
  // The state flag drives the button; the ref stops a second Ctrl+S before React re-renders.
  const savingRef = useRef(false);

  const searchInputRef = useRef<HTMLInputElement>(null);
  const navRef = useRef<HTMLElement>(null);
  const contentPaneRef = useRef<HTMLDivElement>(null);
  const showLicenseNoticesButtonRef = useRef<HTMLButtonElement>(null);
  const lastAutoSelectedQuery = useRef('');

  const baseline = localGames.settings;
  const changedFields = backupDraftChangedFields(settingsDraft, baseline);
  const changeCount = changedFields.length;
  const isDirty = changeCount > 0;

  const requestTopicChange = useCallback(
    (target: SettingsTopic): boolean => {
      if (target === settingsTopic) return false;

      if (isDirty) {
        uiStore.setState((prev) => ({
          ...prev,
          discardBackupConfirmOpen: true,
          pendingSettingsAction: () => {
            uiStore.setState((pending) => ({
              ...pending,
              settingsDraft: null,
              settingsDraftError: null,
            }));
            setSettingsTopic(target);
          },
        }));
        return false;
      }

      setSettingsTopic(target);
      return true;
    },
    [isDirty, settingsTopic],
  );

  const saveDraft = useCallback(async () => {
    if (savingRef.current) return;
    if (!settingsDraft || !isDirty) return;

    const submitted = {
      automatic: backupDraftValue(settingsDraft, baseline, 'automatic'),
      destination: backupDraftValue(settingsDraft, baseline, 'destination'),
      retention: backupDraftValue(settingsDraft, baseline, 'retention'),
    };
    savingRef.current = true;
    setSaving(true);
    uiStore.setState((prev) => ({ ...prev, settingsDraftError: null }));
    try {
      await commands.setLocalBackupSettings(
        submitted.automatic,
        submitted.destination,
        submitted.retention,
      );
      uiStore.setState((prev) => {
        const current = prev.settingsDraft;
        if (!current || current.topic !== 'backups') {
          return { ...prev, settingsDraftError: null };
        }
        // Remove only the values this save wrote. Edits made while the command
        // was in flight stay pending instead of being erased by the late answer.
        const remaining = { ...current.values };
        for (const field of [
          'automatic',
          'destination',
          'retention',
        ] as const) {
          if (remaining[field] === submitted[field]) {
            delete remaining[field];
          }
        }
        return {
          ...prev,
          settingsDraft:
            Object.keys(remaining).length > 0
              ? { topic: 'backups', values: remaining }
              : null,
          settingsDraftError: null,
        };
      });
      pushToast('success', t('settings.unsaved.saved'));
    } catch (error) {
      const detail =
        typeof error === 'string'
          ? error
          : (error as { message?: unknown } | null)?.message;
      const message =
        typeof detail === 'string' && detail.length > 0
          ? detail
          : t('settings.unsaved.saveFailed');
      uiStore.setState((prev) => {
        // A discarded draft has no field to mark; a newer destination edit is
        // not the value that failed, so only the submitted one keeps the error.
        if (!prev.settingsDraft) return prev;
        if (
          prev.settingsDraft.values.destination !== undefined &&
          prev.settingsDraft.values.destination !== submitted.destination
        ) {
          return prev;
        }
        return { ...prev, settingsDraftError: message };
      });
      pushToast('danger', t('settings.unsaved.saveFailed'));
    } finally {
      savingRef.current = false;
      setSaving(false);
    }
  }, [baseline, isDirty, settingsDraft, t]);

  // The search is component state: closing Settings forgets it.
  useEffect(() => {
    if (!settingsOpen) {
      setSearchQuery('');
      setSaving(false);
    }
  }, [settingsOpen]);

  // Reset the panel scroll when the topic changes.
  useEffect(() => {
    if (contentPaneRef.current) {
      contentPaneRef.current.scrollTop = 0;
    }
  }, [settingsTopic]);

  // A non-empty query selects the first matching topic through the same
  // discard-aware path as a click (D2). Only a query change re-selects, so the
  // user can still pick another matching topic afterwards.
  useEffect(() => {
    if (!settingsOpen) {
      lastAutoSelectedQuery.current = '';
      return;
    }
    const normalized = normalizeSettingsSearch(searchQuery);
    if (normalized.length === 0) {
      lastAutoSelectedQuery.current = '';
      return;
    }
    if (discardConfirmOpen) return;
    if (lastAutoSelectedQuery.current === normalized) return;

    const matches = settingsTopicsForSearch(searchQuery, t);
    if (matches.length === 0) return;

    lastAutoSelectedQuery.current = normalized;
    requestTopicChange(matches[0].id);
  }, [
    discardConfirmOpen,
    requestTopicChange,
    searchQuery,
    settingsOpen,
    t,
  ]);

  // Bring the first highlighted row into view.
  useEffect(() => {
    if (!settingsOpen) return;
    if (normalizeSettingsSearch(searchQuery).length === 0) return;
    const match = contentPaneRef.current?.querySelector<HTMLElement>(
      '[data-search-match="true"]',
    );
    match?.scrollIntoView?.({ block: 'nearest' });
  }, [searchQuery, settingsOpen, settingsTopic]);

  // Ctrl/Cmd+F focuses the settings search (the library shortcut is inert behind
  // the modal) and Ctrl/Cmd+S saves the pending group while the bar is visible.
  useEffect(() => {
    if (!settingsOpen) return undefined;

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;

      const key = event.key.toLowerCase();
      if (key === 'f' && !event.shiftKey) {
        if (discardConfirmOpen || licenseModalOpen) return;
        event.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
        return;
      }
      if (key === 's' && !event.shiftKey && isDirty) {
        event.preventDefault();
        void saveDraft();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [
    discardConfirmOpen,
    isDirty,
    licenseModalOpen,
    saveDraft,
    settingsOpen,
  ]);

  const handleRequestClose = () => {
    if (isDirty) {
      uiStore.setState((prev) => ({
        ...prev,
        discardBackupConfirmOpen: true,
        pendingSettingsAction: () => closeSettingsModal(),
      }));
      return;
    }
    closeSettingsModal();
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
      settingsDraft: null,
      settingsDraftError: null,
      discardBackupConfirmOpen: false,
      pendingSettingsAction: null,
    }));
    pendingAction?.();
  };

  const handleRevert = () => {
    uiStore.setState((prev) => ({
      ...prev,
      settingsDraft: null,
      settingsDraftError: null,
    }));
  };

  const handleCloseLicenseModal = () => {
    uiStore.setState((prev) => ({
      ...prev,
      licenseModalOpen: false,
    }));
    showLicenseNoticesButtonRef.current?.focus();
  };

  const handleNavKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    if (event.target instanceof HTMLInputElement) return;

    const nav = navRef.current;
    if (!nav) return;
    const wrappers = Array.from(
      nav.querySelectorAll<HTMLElement>('[data-settings-topic]'),
    );
    const buttons = wrappers
      .map((wrapper) => wrapper.querySelector<HTMLButtonElement>('button'))
      .filter((button): button is HTMLButtonElement => button !== null);
    if (buttons.length === 0) return;

    const activeIndex = buttons.findIndex(
      (button) => button === document.activeElement,
    );
    const delta = event.key === 'ArrowDown' ? 1 : -1;
    const nextIndex =
      activeIndex < 0
        ? delta > 0
          ? 0
          : buttons.length - 1
        : (activeIndex + delta + buttons.length) % buttons.length;
    const nextButton = buttons[nextIndex];
    const nextTopic = wrappers[nextIndex]?.dataset
      .settingsTopic as SettingsTopic | undefined;
    if (!nextButton || !nextTopic) return;

    event.preventDefault();
    if (requestTopicChange(nextTopic)) {
      nextButton.focus();
    }
  };

  if (!settingsOpen) {
    return null;
  }

  const visibleGroups = settingsNavGroupsForSearch(searchQuery, t);
  const pluginCount = Array.isArray(plugins) ? plugins.length : undefined;
  const pendingChanges = changedFields.map((field) =>
    t(BACKUP_DRAFT_FIELD_LABEL_KEYS[field]),
  );

  const renderActivePanel = () => {
    switch (settingsTopic) {
      case 'account':
        return <AccountPanel />;
      case 'stores':
        return <StoresPanel />;
      case 'drmFree':
        return <DrmFreePanel />;
      case 'backups':
        return <BackupsPanel />;
      case 'general':
        return <GeneralPanel />;
      case 'tools':
        return <ToolsPanel />;
      case 'plugins':
        return <PluginsPanel />;
      case 'about':
        return (
          <AboutPanel showLicenseNoticesButtonRef={showLicenseNoticesButtonRef} />
        );
      default:
        return <AccountPanel />;
    }
  };

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
        <FloatingModal.Content className="flex flex-col overflow-hidden rounded-dialog border border-border-overlay !bg-canvas shadow-dialog !shadow-alert">
          <FloatingModal.Header className="!h-[48px] !gap-xs !py-0 border-b border-border-subtle bg-chrome pr-xs pl-[14px]">
            <IconSettings
              size={16}
              stroke={2}
              aria-hidden="true"
              className="shrink-0 text-text-secondary"
            />
            <FloatingModal.Title className="!text-sm font-semibold text-text-primary">
              {t('settings.shell.title')}
            </FloatingModal.Title>
            <Kbd aria-hidden="true" className="ml-auto">
              Esc
            </Kbd>
            <FloatingModal.CloseButton
              aria-label={t('settings.shell.close')}
              className="!h-[32px] !w-[32px] rounded-control"
            />
          </FloatingModal.Header>

          <div className="flex min-h-0 flex-1">
            <nav
              ref={navRef}
              aria-label={t('settings.shell.title')}
              onKeyDown={handleNavKeyDown}
              className="flex w-settings-nav shrink-0 flex-col gap-md overflow-y-auto border-r border-border-subtle bg-chrome p-sm"
            >
              <SettingsSearch
                ref={searchInputRef}
                value={searchQuery}
                onChange={setSearchQuery}
              />

              {visibleGroups.length === 0 ? (
                <p className="px-xs text-xs text-text-dimmed">
                  {t('settings.shell.searchEmpty')}
                </p>
              ) : null}

              {visibleGroups.map((group) => (
                <div key={group.id} className="flex flex-col gap-[2px]">
                  <div className="flex px-xs pb-[4px]">
                    <span className="text-overline font-semibold uppercase tracking-label text-text-muted">
                      {t(group.labelKey)}
                    </span>
                  </div>
                  {group.topics.map((topic) => (
                    <div key={topic.id} data-settings-topic={topic.id}>
                      <NavItem
                        icon={topic.icon}
                        label={t(topic.titleKey)}
                        active={topic.id === settingsTopic}
                        onClick={() => requestTopicChange(topic.id)}
                        warning={
                          topic.id === 'stores'
                            ? storesAttentionLabel
                            : undefined
                        }
                        count={topic.id === 'plugins' ? pluginCount : undefined}
                      />
                    </div>
                  ))}
                </div>
              ))}

              <div className="mt-auto truncate text-caption text-text-muted">
                {t('settings.shell.footer', {
                  version: appInfo?.version ?? '0.1.0',
                })}
              </div>
            </nav>

            <section className="relative flex min-w-0 flex-1 flex-col">
              <div
                ref={contentPaneRef}
                className={cx(
                  'settings-scroll-content flex min-h-0 flex-1 flex-col gap-lg overflow-y-auto px-xl pt-[28px]',
                  isDirty ? 'pb-[88px]' : 'pb-xl',
                )}
              >
                <SettingsSearchProvider query={searchQuery}>
                  {renderActivePanel()}
                </SettingsSearchProvider>
              </div>

              <ScrollFade scrollerRef={contentPaneRef} raised={isDirty} />

              {isDirty ? (
                <UnsavedChangesBar
                  data-testid="settings-unsaved-bar"
                  count={changeCount}
                  saving={saving}
                  onRevert={handleRevert}
                  onSave={() => void saveDraft()}
                />
              ) : null}
            </section>
          </div>

          <FloatingModal.ResizeHandle />
        </FloatingModal.Content>
      </FloatingModal.Root>

      {discardConfirmOpen ? (
        <DiscardChangesDialog
          pendingChanges={pendingChanges}
          onCancel={handleCancelDiscard}
          onConfirm={handleConfirmDiscard}
        />
      ) : null}

      {licenseModalOpen ? (
        <LicenseNoticesModal onClose={handleCloseLicenseModal} />
      ) : null}
    </>
  );
};
