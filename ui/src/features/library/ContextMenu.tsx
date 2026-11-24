import React, { useContext, useEffect, useState } from 'react';
import { MantineContext, MantineProvider, Menu } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import {
  IconBookmark,
  IconCheck,
  IconChevronRight,
  IconDownload,
  IconEye,
  IconEyeOff,
  IconFolderOpen,
  IconLink,
  IconMapPin,
  IconPlayerPlay,
  IconPlus,
  IconSettings,
  IconShieldCheck,
  IconStar,
  IconStarFilled,
  IconTool,
  IconTrash,
  IconTrophy,
} from '@tabler/icons-react';
import {
  commands,
  coverAssetUrl,
  getMatchedGameId,
  useLibraryView,
  type EntryStatus,
  type PluginInfo,
  type Reason,
} from '../../api';
import {
  Kbd,
  MenuSurface,
  menuClassNames,
  menuItemDangerClass,
  menuItemReasonClass,
  menuItemStackClass,
} from '../../components/ui';
import { closeContextMenu, pushToast, uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { AchievementListModal } from './AchievementListModal';
import { contextMenuAnchorStyle } from './contextMenuAnchor';
import { ENTRY_STATUSES, entryRegistrationId, isDrmFreeKey } from './detail/format';
import { summarizeSelection } from './detail/multiSelection';
import { formatI18n, formatReason, formatStatus } from './formatters';

/** Menu width from `raw/06-L4-menus-popovers.txt`. */
const MENU_WIDTH = 280;

/** The disabled reason of an engine availability, if it is disabled. */
function policyReason(value: unknown): Reason | undefined {
  return typeof value === 'object' && value != null && 'disabled' in value
    ? ((value as { disabled: Reason }).disabled)
    : undefined;
}

interface ItemReasonProps {
  label: React.ReactNode;
  reason?: string;
}

function ItemReason({ label, reason }: ItemReasonProps) {
  return (
    <span className={menuItemStackClass}>
      <span>{label}</span>
      {reason && <span className={menuItemReasonClass}>{reason}</span>}
    </span>
  );
}

export const ContextMenu: React.FC = () => {
  const contextMenu = useUiStore((s) => s.contextMenu);
  const library = useLibraryView();
  const { t } = useI18n();
  const mantineContext = useContext(MantineContext);
  const [achievements, setAchievements] = useState<{
    key: string;
    title: string;
    providerName: string;
  } | null>(null);
  const [statusOpen, setStatusOpen] = useState(false);
  const [collectionsOpen, setCollectionsOpen] = useState(false);
  const [extensionsOpen, setExtensionsOpen] = useState(false);
  const [extensionActions, setExtensionActions] = useState<{
    play: PluginInfo[];
    install: PluginInfo[];
  }>({ play: [], install: [] });

  const isOpen = contextMenu.open && contextMenu.entryKey != null;
  const entryKey = contextMenu.entryKey ?? '';
  const selected = library?.selected ?? [];
  const isMulti = selected.length > 1;
  const rows = library?.rows ?? [];
  const summary = summarizeSelection(selected, rows);
  const row = rows.find((candidate) => candidate.key === entryKey) ?? null;
  const detail = library?.detail && library.detail.row.key === entryKey ? library.detail : null;

  useEffect(() => {
    if (!isOpen || !row || isMulti || getMatchedGameId(row.status) == null) {
      if (extensionActions.play.length > 0 || extensionActions.install.length > 0) {
        setExtensionActions({ play: [], install: [] });
      }
      return;
    }
    let cancelled = false;
    const load = async () => {
      const [play, install] = await Promise.all([
        // A provider without its own launch rule (for example a ROM store) relies on an
        // extension for Play, so an installed row must still offer the extension action.
        row.installed
          ? commands.availablePluginExtensions('play')
          : Promise.resolve([]),
        !row.installed
          ? commands.availablePluginExtensions('install')
          : Promise.resolve([]),
      ]);
      // The common no-extension result is already the initial state; avoid a needless
      // asynchronous render for every ordinary library menu opening.
      if (
        !cancelled &&
        (play.length > 0 || install.length > 0 || extensionActions.play.length > 0 || extensionActions.install.length > 0)
      ) {
        setExtensionActions({ play, install });
      }
    };
    void load().catch(() => {
      if (!cancelled) setExtensionActions({ play: [], install: [] });
    });
    return () => {
      cancelled = true;
    };
  }, [isOpen, isMulti, row?.key, row?.installed, row?.status]);

  useEffect(() => {
    if (!isOpen) {
      setStatusOpen(false);
      setCollectionsOpen(false);
      setExtensionsOpen(false);
    }
  }, [isOpen]);

  // Close on Escape. Menu handles outside clicks and item dismissal.
  useEffect(() => {
    if (!isOpen) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        closeContextMenu();
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
    };
  }, [isOpen]);

  const achievementModal = achievements ? (
    <AchievementListModal
      opened
      onClose={() => setAchievements(null)}
      entryKey={achievements.key}
      gameTitle={achievements.title}
      providerName={achievements.providerName}
    />
  ) : null;

  const wrap = (content: React.ReactNode) =>
    mantineContext ? (
      content
    ) : (
      <MantineProvider theme={appTheme} defaultColorScheme="dark">{content}</MantineProvider>
    );

  if (!isOpen && !achievements) return null;
  if (isOpen && !row && !isMulti) return wrap(achievementModal);

  // Position adjustments to prevent overflow. Right-click coordinates arrive from the row
  // handler; the keyboard helper supplies the focused row's centre instead of (0, 0).
  const anchorStyle = contextMenuAnchorStyle({
    x: contextMenu.x,
    y: contextMenu.y,
    menuWidth: MENU_WIDTH,
  });

  const close = () => closeContextMenu();

  const single = !isMulti ? row : null;
  const actions = library?.actions;
  const gameId = single ? getMatchedGameId(single.status) : null;
  const inLibrary = single?.gamenode != null;
  const matched = gameId != null;
  const isDrmFree = single != null && isDrmFreeKey(single.key);
  const installPath = single && single.installed ? (detail?.installDir ?? '') : '';
  const canMatch = actions?.matchGame === 'available';
  const matchReason = formatReason(policyReason(actions?.matchGame), t);
  const addReason = formatReason(policyReason(actions?.addToGamenode), t);
  const installReason = formatReason(policyReason(actions?.install), t);
  const playReason = formatReason(policyReason(actions?.play), t);
  const showInstall =
    single != null &&
    !single.installed &&
    actions != null &&
    policyReason(actions.install) !== 'noInstallRule';
  const toolsReason = single?.running ? formatReason('running', t) : '';
  const changeMatch = actions?.changeMatch ?? false;
  const extensionChoices = [
    ...extensionActions.play.map((plugin) => ({ plugin, operation: 'play' as const })),
    ...extensionActions.install.map((plugin) => ({ plugin, operation: 'install' as const })),
  ];

  const startWithExtension = (plugin: PluginInfo, operation: 'play' | 'install') => {
    if (!single) return;
    void commands.startExtensionAction(single.key, plugin.pluginId, operation).catch(() => {
      pushToast('danger', t('Library::The extension could not be started.'));
    });
    close();
  };

  const openPage = (page: { game: { id: number } }) => {
    void commands.openGameNodePage(page).catch(() => {
      pushToast('danger', t('Library::The link could not be opened.'));
    });
    close();
  };

  const toggleFavorite = () => {
    if (!single || gameId == null) return;
    commands.libraryInput({
      setFavorite: {
        key: single.key,
        favorite: !single.isFavorite,
        now: Math.floor(Date.now() / 1000),
      },
    });
    close();
  };

  const matchedRows = summary.rows.filter(
    (candidate) => getMatchedGameId(candidate.status) != null,
  );
  const matchedKeys = matchedRows.map((candidate) => candidate.key);
  const libraryKeys = summary.rows
    .filter((candidate) => candidate.gamenode != null)
    .map((candidate) => candidate.key);
  const allFavorites = matchedRows.length > 0 && matchedRows.every((candidate) => candidate.isFavorite);
  const allHidden = summary.rows.length > 0 && summary.rows.every((candidate) => candidate.hidden);
  const canChangeStatus = summary.rows.some(
    (candidate) => candidate.gamenode != null && candidate.gamenode.status != null,
  );

  const toggleBatchFavorite = () => {
    if (matchedKeys.length === 0) return;
    commands.libraryInput({
      setFavorites: {
        keys: matchedKeys,
        favorite: !allFavorites,
        now: Math.floor(Date.now() / 1000),
      },
    });
    close();
  };

  const hideKeys = single ? [single.key] : selected;
  const hideTarget = single ? !single.hidden : !allHidden;

  const collectionKeys = single ? (inLibrary ? [single.key] : []) : libraryKeys;
  const collections = library?.collections ?? [];
  const collectionsDisabledReason =
    collectionKeys.length === 0
      ? t('Library::Add to GameNode first')
      : collections.length === 0
        ? t('Library::No collections')
        : '';

  const collectionChecked = (collectionId: string) =>
    collectionKeys.length > 0 &&
    collectionKeys.every((key) => {
      const selectedRow = rows.find((candidate) => candidate.key === key);
      return selectedRow?.gamenode?.collectionIds.includes(collectionId) ?? false;
    });

  const addToCollection = (collectionId: string) => {
    if (collectionKeys.length === 0) return;
    commands.libraryInput({ addToCollection: { keys: collectionKeys, collectionId } });
    close();
  };

  const changeStatus = (status: EntryStatus) => {
    commands.libraryInput({ setStatus: { keys: isMulti ? selected : [entryKey], status } });
    close();
  };

  const setHidden = () => {
    commands.libraryInput({ setHidden: { keys: hideKeys, hidden: hideTarget } });
    close();
  };

  const headerTitle = single
    ? single.title
    : formatI18n(t('Library::{} games selected'), selected.length);
  const headerSubtitle = single
    ? formatI18n(
        t(single.installed ? 'Library::{} · Installed' : 'Library::{} · Not installed'),
        single.providerName,
      )
    : summary.stores.join(' · ');

  const menu = (
    <MenuSurface
      opened
      onClose={closeContextMenu}
      position="bottom-start"
      offset={0}
      floatingStrategy="fixed"
      zIndex={1000}
      trapFocus={false}
      returnFocus={false}
      closeOnEscape={false}
      transitionProps={{ duration: 0 }}
      classNames={{ dropdown: 'w-[280px]' }}
    >
      {/* Zero-size anchor at the right-click / keyboard-open coordinates. */}
      <Menu.Target>
        <div data-testid="library-context-menu-anchor" aria-hidden style={anchorStyle} />
      </Menu.Target>

      <Menu.Dropdown data-testid="library-context-menu" aria-label="Game actions">
        <div className="flex items-center gap-[10px] px-[10px] py-[8px]">
          <div className="flex h-[32px] w-[24px] shrink-0 items-center justify-center overflow-hidden rounded-xs bg-surface">
            {single && coverAssetUrl(single.coverFile) ? (
              <img
                src={coverAssetUrl(single.coverFile) ?? undefined}
                alt=""
                loading="lazy"
                decoding="async"
                className="h-full w-full object-cover"
              />
            ) : null}
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-[13px] font-semibold text-text-primary">
              {headerTitle}
            </div>
            <div className="truncate text-[11px] font-normal text-text-dimmed">
              {headerSubtitle}
            </div>
          </div>
        </div>

        <Menu.Divider />

        {single && (
          <Menu.Item
            data-testid="context-menu-play"
            disabled={actions?.play !== 'available'}
            onClick={() => {
              if (actions?.play !== 'available') return;
              commands.launchSelected('play');
              close();
            }}
            leftSection={<IconPlayerPlay size={16} stroke={2} aria-hidden />}
            rightSection={!playReason ? <Kbd>Enter</Kbd> : undefined}
          >
            <ItemReason label={t('Library::Play')} reason={playReason} />
          </Menu.Item>
        )}

        {showInstall && (
          <Menu.Item
            data-testid="context-menu-install"
            disabled={actions?.install !== 'available'}
            onClick={() => {
              if (actions?.install !== 'available') return;
              commands.launchSelected('install');
              close();
            }}
            leftSection={<IconDownload size={16} stroke={2} aria-hidden />}
          >
            <ItemReason label={t('Library::Install')} reason={installReason} />
          </Menu.Item>
        )}

        {single && matched && extensionChoices.length > 0 && (
          <Menu.Sub
            opened={extensionsOpen}
            onChange={setExtensionsOpen}
            position="right-start"
            offset={4}
            floatingStrategy="fixed"
          >
            <Menu.Sub.Target>
              <Menu.Item
                data-testid="context-menu-start-with-extension"
                closeMenuOnClick={false}
                onClick={() => setExtensionsOpen((open) => !open)}
                leftSection={<IconPlayerPlay size={16} stroke={2} aria-hidden />}
                rightSection={<IconChevronRight size={14} aria-hidden />}
              >
                {t('Library::Start with extension')}
              </Menu.Item>
            </Menu.Sub.Target>
            <Menu.Sub.Dropdown classNames={{ dropdown: menuClassNames.dropdown }}>
              {extensionChoices.map(({ plugin, operation }) => (
                <Menu.Sub.Item
                  key={`${plugin.pluginId}:${operation}`}
                  data-testid={`context-menu-extension-${plugin.pluginId}-${operation}`}
                  classNames={{
                    item: menuClassNames.item,
                    itemLabel: menuClassNames.itemLabel,
                    itemIndicator: menuClassNames.itemIndicator,
                  }}
                  onClick={() => startWithExtension(plugin, operation)}
                >
                  {formatI18n(
                    t('Library::{} · {}'),
                    plugin.name,
                    t(operation === 'play' ? 'Library::Play' : 'Library::Install'),
                  )}
                </Menu.Sub.Item>
              ))}
            </Menu.Sub.Dropdown>
          </Menu.Sub>
        )}

        {single?.installed && installPath !== '' && (
          <Menu.Item
            data-testid="context-menu-open-folder"
            onClick={() => {
              void commands.openInstallFolder(installPath);
              close();
            }}
            leftSection={<IconFolderOpen size={16} stroke={2} aria-hidden />}
          >
            {t('Library::Open install folder')}
          </Menu.Item>
        )}

        {(single != null || isMulti) && <Menu.Divider />}

        {single && (
          <>
            {inLibrary && matched ? (
              <Menu.Item
                data-testid="context-menu-edit-gamenode"
                onClick={() => openPage({ game: { id: gameId } })}
                leftSection={<IconLink size={16} stroke={2} aria-hidden />}
              >
                {t('Library::Edit on GameNode…')}
              </Menu.Item>
            ) : (
              <Menu.Item
                data-testid="context-menu-add"
                disabled={actions?.addToGamenode !== 'available'}
                onClick={() => {
                  if (actions?.addToGamenode !== 'available') return;
                  commands.libraryInput('openAdd');
                  close();
                }}
                leftSection={<IconPlus size={16} stroke={2} aria-hidden />}
              >
                <ItemReason
                  label={t('Library::Add to GameNode…')}
                  reason={
                    actions?.addToGamenode !== 'available'
                      ? addReason || formatReason(matched ? undefined : 'notMatched', t)
                      : undefined
                  }
                />
              </Menu.Item>
            )}

            <Menu.Item
              data-testid="context-menu-match"
              disabled={!canMatch}
              onClick={() => {
                if (!canMatch) return;
                commands.libraryInput('openMatch');
                close();
              }}
              leftSection={<IconLink size={16} stroke={2} aria-hidden />}
            >
              <ItemReason
                label={changeMatch ? t('Library::Change match…') : t('Library::Match…')}
                reason={matchReason}
              />
            </Menu.Item>

            {matched && (
              <Menu.Item
                data-testid="context-menu-favorite"
                aria-pressed={single.isFavorite}
                onClick={toggleFavorite}
                leftSection={
                  single.isFavorite ? (
                    <IconStarFilled size={16} aria-hidden className="text-warning" />
                  ) : (
                    <IconStar size={16} stroke={2} aria-hidden />
                  )
                }
                rightSection={<Kbd>F</Kbd>}
              >
                {single.isFavorite
                  ? t('Library::Remove from favorites')
                  : t('Library::Add to favorites')}
              </Menu.Item>
            )}
          </>
        )}

        {isMulti && (
          <>
            <Menu.Item
              data-testid="context-menu-add"
              disabled={actions?.addToGamenode !== 'available'}
              onClick={() => {
                if (actions?.addToGamenode !== 'available') return;
                commands.libraryInput('openAdd');
                close();
              }}
              leftSection={<IconPlus size={16} stroke={2} aria-hidden />}
            >
              <ItemReason
                label={t('Library::Add to GameNode…')}
                reason={actions?.addToGamenode !== 'available' ? addReason : undefined}
              />
            </Menu.Item>

            <Menu.Sub
              opened={statusOpen}
              onChange={setStatusOpen}
              position="right-start"
              offset={4}
              floatingStrategy="fixed"
            >
              <Menu.Sub.Target>
                <Menu.Item
                  data-testid="context-menu-change-status"
                  disabled={!canChangeStatus}
                  closeMenuOnClick={false}
                  onClick={() => setStatusOpen((open) => !open)}
                  leftSection={<IconBookmark size={16} stroke={2} aria-hidden />}
                  rightSection={<IconChevronRight size={14} aria-hidden />}
                >
                  <ItemReason
                    label={t('Library::Change status')}
                    reason={canChangeStatus ? undefined : t('Library::Add to GameNode first')}
                  />
                </Menu.Item>
              </Menu.Sub.Target>
              <Menu.Sub.Dropdown classNames={{ dropdown: menuClassNames.dropdown }}>
                {ENTRY_STATUSES.map((status) => (
                  <Menu.Sub.Item
                    key={status}
                    data-testid={`context-menu-status-${status}`}
                    classNames={{
                      item: menuClassNames.item,
                      itemLabel: menuClassNames.itemLabel,
                      itemSection: menuClassNames.itemSection,
                      itemIndicator: menuClassNames.itemIndicator,
                    }}
                    onClick={() => changeStatus(status)}
                  >
                    {formatStatus(status, t)}
                  </Menu.Sub.Item>
                ))}
              </Menu.Sub.Dropdown>
            </Menu.Sub>

            <Menu.Item
              data-testid="context-menu-favorite"
              disabled={matchedKeys.length === 0}
              onClick={toggleBatchFavorite}
              leftSection={
                allFavorites ? (
                  <IconStarFilled size={16} aria-hidden className="text-warning" />
                ) : (
                  <IconStar size={16} stroke={2} aria-hidden />
                )
              }
            >
              <ItemReason
                label={
                  allFavorites
                    ? t('Library::Remove from favorites')
                    : t('Library::Add to favorites')
                }
                reason={matchedKeys.length === 0 ? formatReason('notMatched', t) : undefined}
              />
            </Menu.Item>
          </>
        )}

        {collectionsDisabledReason !== '' ? (
          <Menu.Item
            data-testid="context-menu-collection-disabled"
            disabled
            leftSection={<IconBookmark size={16} stroke={2} aria-hidden />}
            rightSection={<IconChevronRight size={14} aria-hidden />}
          >
            <ItemReason
              label={t('Library::Add to collection')}
              reason={collectionsDisabledReason}
            />
          </Menu.Item>
        ) : (
          <Menu.Sub
            opened={collectionsOpen}
            onChange={setCollectionsOpen}
            position="right-start"
            offset={4}
            floatingStrategy="fixed"
          >
            <Menu.Sub.Target>
              <Menu.Item
                data-testid="context-menu-collection"
                closeMenuOnClick={false}
                onClick={() => setCollectionsOpen((open) => !open)}
                leftSection={<IconBookmark size={16} stroke={2} aria-hidden />}
                rightSection={<IconChevronRight size={14} aria-hidden />}
              >
                {t('Library::Add to collection')}
              </Menu.Item>
            </Menu.Sub.Target>
            <Menu.Sub.Dropdown classNames={{ dropdown: menuClassNames.dropdown }}>
              {collections.map((collection) => {
                const checked = collectionChecked(collection.id);
                return (
                  <Menu.Sub.Item
                    key={collection.id}
                    data-testid={`context-menu-collection-${collection.id}`}
                    classNames={{
                      item: menuClassNames.item,
                      itemLabel: menuClassNames.itemLabel,
                      itemSection: menuClassNames.itemSection,
                      itemIndicator: menuClassNames.itemIndicator,
                    }}
                    disabled={checked}
                    onClick={() => addToCollection(collection.id)}
                    leftSection={
                      checked ? <IconCheck size={16} aria-hidden /> : <span className="w-[16px]" />
                    }
                  >
                    {collection.name}
                  </Menu.Sub.Item>
                );
              })}
            </Menu.Sub.Dropdown>
          </Menu.Sub>
        )}

        {single && (
          <>
            <Menu.Divider />
            <Menu.Item
              data-testid="context-menu-tools"
              disabled={single.running}
              onClick={() => {
                if (single.running) return;
                commands.openTools(entryKey);
                close();
              }}
              leftSection={<IconTool size={16} stroke={2} aria-hidden />}
            >
              <ItemReason label={t('Library::Game tools…')} reason={toolsReason} />
            </Menu.Item>

            {isDrmFree && (
              <Menu.Item
                data-testid="context-menu-save-protection"
                onClick={() => {
                  uiStore.setState((prev) => ({ ...prev, saveProtectionEntryKey: entryKey }));
                  commands.loadLocalHistory(entryRegistrationId(entryKey));
                  close();
                }}
                leftSection={<IconShieldCheck size={16} stroke={2} aria-hidden />}
              >
                {t('Library::Local save protection')}
              </Menu.Item>
            )}

            {detail?.achievementBackend && (
              <Menu.Item
                data-testid="context-menu-achievements"
                onClick={() => {
                  setAchievements({
                    key: entryKey,
                    title: single.title,
                    providerName: single.providerName,
                  });
                  close();
                }}
                leftSection={<IconTrophy size={16} stroke={2} aria-hidden />}
              >
                {t('Library::View achievements')}
              </Menu.Item>
            )}
          </>
        )}

        {(single != null || isMulti) && (
          <>
            <Menu.Divider />
            <Menu.Item
              data-testid={hideTarget ? 'context-menu-hide' : 'context-menu-show'}
              onClick={setHidden}
              leftSection={
                hideTarget ? (
                  <IconEyeOff size={16} stroke={2} aria-hidden />
                ) : (
                  <IconEye size={16} stroke={2} aria-hidden />
                )
              }
            >
              {hideTarget ? t('Library::Hide from library') : t('Library::Show in library')}
            </Menu.Item>
          </>
        )}

        {isDrmFree && single && (
          <>
            <Menu.Divider />
            <Menu.Item
              data-testid="context-menu-settings"
              onClick={() => {
                uiStore.setState((prev) => ({ ...prev, localEditorEntryKey: entryKey }));
                close();
              }}
              leftSection={<IconSettings size={16} stroke={2} aria-hidden />}
            >
              {t('Library::Game settings…')}
            </Menu.Item>
            <Menu.Item
              data-testid="context-menu-locate"
              onClick={() => {
                void commands.pickLocalExecutable(entryRegistrationId(entryKey));
                close();
              }}
              leftSection={<IconMapPin size={16} stroke={2} aria-hidden />}
            >
              {t('Library::Locate executable…')}
            </Menu.Item>
            <Menu.Item
              data-testid="context-menu-remove"
              onClick={() => {
                uiStore.setState((prev) => ({ ...prev, removeModalEntryKey: entryKey }));
                close();
              }}
              className={menuItemDangerClass}
              leftSection={<IconTrash size={16} stroke={2} aria-hidden />}
            >
              {t('Library::Remove from library…')}
            </Menu.Item>
          </>
        )}
      </Menu.Dropdown>
    </MenuSurface>
  );

  return wrap(
    <>
      {isOpen && menu}
      {achievementModal}
    </>,
  );
};
