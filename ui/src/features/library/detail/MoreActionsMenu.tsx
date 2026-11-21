import React from 'react';
import { Menu } from '@mantine/core';
import { useQuery } from '@tanstack/react-query';
import {
  IconBookmark,
  IconCirclePlay,
  IconPlayerStop,
  IconDots,
  IconDownload,
  IconFolderOpen,
  IconLink,
  IconSettings,
  IconShieldCheck,
  IconStar,
  IconStarFilled,
  IconTrash,
  IconTrophy,
  IconEyeOff,
  IconEye,
  IconTool,
} from '@tabler/icons-react';
import {
  commands,
  type Actions,
  type DetailView,
  type GameToolView,
} from '../../../api';
import {
  IconButton,
  MenuSurface,
  menuItemDangerClass,
  menuItemReasonClass,
  menuItemStackClass,
} from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { uiStore } from '../../../store/uiStore';
import { formatReason } from '../formatters';
import { entryRegistrationId, toolSummary } from './format';

export interface MoreActionsMenuProps {
  detail: DetailView;
  actions: Actions;
  installPath: string | null;
  isRunning: boolean;
  isDrmFree: boolean;
  inLibrary: boolean;
  gameId: number | null;
  tools: GameToolView[];
  onViewAchievements: () => void;
}

/**
 * The detail pane's more-actions menu. It reuses the existing engine inputs and commands; the
 * richer shared ContextMenu is built later in the wave and is deliberately not imported here.
 */
export function MoreActionsMenu({
  detail,
  actions,
  installPath,
  isRunning,
  isDrmFree,
  inLibrary,
  gameId,
  tools,
  onViewAchievements,
}: MoreActionsMenuProps) {
  const { t } = useI18n();
  const [opened, setOpened] = React.useState(false);
  const [stoppingLaunch, setStoppingLaunch] = React.useState(false);
  const [stopLaunchFailed, setStopLaunchFailed] = React.useState(false);
  const row = detail.row;
  const customLaunches = useQuery({
    queryKey: ['custom-launches', row.key],
    queryFn: () => commands.customLaunchIds(row.key),
    enabled: opened,
    staleTime: 0,
    refetchInterval: opened ? 2000 : false,
  });
  const playReady = actions.play === 'available';
  const installHasRule =
    actions.install === 'available' ||
    (typeof actions.install === 'object' && actions.install.disabled !== 'noInstallRule');
  const matchReady = actions.matchGame === 'available';
  const canFavorite = gameId != null;
  const installedTools = toolSummary(tools);

  const play = (kind: 'play' | 'install') => commands.launchSelected(kind);
  const editOnGameNode = () => {
    if (gameId != null) commands.openGameNodePage({ game: { id: gameId } });
  };

  return (
    <MenuSurface
      opened={opened}
      onChange={setOpened}
      position="bottom-end"
      width={220}
      withinPortal
      hideDetached={false}
      transitionProps={{ duration: 0 }}
    >
      <Menu.Target>
        <IconButton
          icon={IconDots}
          label={t('Detail::More actions')}
          variant="outlined"
          withTooltip={false}
          data-testid="detail-more-btn"
        />
      </Menu.Target>
      <Menu.Dropdown>
        <Menu.Item
          leftSection={<IconCirclePlay size={16} stroke={2} aria-hidden="true" />}
          disabled={!playReady}
          onClick={() => play('play')}
        >
          {t('Library::Play')}
          {!playReady && (
            <span className={menuItemStackClass}>
              <span className={menuItemReasonClass}>
                {formatReason(typeof actions.play === 'object' ? actions.play.disabled : null, t)}
              </span>
            </span>
          )}
        </Menu.Item>
        {(customLaunches.data?.length ?? 0) > 0 && (
          <Menu.Item
            leftSection={<IconPlayerStop size={16} stroke={2} aria-hidden="true" />}
            disabled={stoppingLaunch}
            closeMenuOnClick={false}
            onClick={() => {
              setStoppingLaunch(true);
              setStopLaunchFailed(false);
              void Promise.all((customLaunches.data ?? []).map((id) => commands.customLaunchStop(id)))
                .catch(() => setStopLaunchFailed(true))
                .finally(() => {
                  void customLaunches.refetch();
                  setStoppingLaunch(false);
                });
            }}
          >
            {t('Detail::Stop custom launch')}
            {stopLaunchFailed && <span className={menuItemReasonClass}>{t('Detail::Could not stop custom launch')}</span>}
          </Menu.Item>
        )}
        {installHasRule && (
          <Menu.Item
            leftSection={<IconDownload size={16} stroke={2} aria-hidden="true" />}
            disabled={actions.install !== 'available'}
            onClick={() => play('install')}
          >
            {t('Library::Install')}
          </Menu.Item>
        )}
        {row.installed && installPath && (
          <Menu.Item
            leftSection={<IconFolderOpen size={16} stroke={2} aria-hidden="true" />}
            onClick={() => void commands.openInstallFolder(installPath)}
          >
            {t('Library::Open install folder')}
          </Menu.Item>
        )}

        <Menu.Divider />

        {gameId != null &&
          (inLibrary ? (
            <Menu.Item
              leftSection={<IconLink size={16} stroke={2} aria-hidden="true" />}
              onClick={editOnGameNode}
            >
              {t('Library::Edit on GameNode…')}
            </Menu.Item>
          ) : (
            <Menu.Item
              leftSection={<IconBookmark size={16} stroke={2} aria-hidden="true" />}
              disabled={actions.addToGamenode !== 'available'}
              onClick={() => commands.libraryInput('openAdd')}
            >
              <span className={menuItemStackClass}>
                <span>{t('Library::Add to GameNode…')}</span>
                {actions.addToGamenode !== 'available' && (
                  <span className={menuItemReasonClass}>
                    {formatReason(
                      typeof actions.addToGamenode === 'object'
                        ? actions.addToGamenode.disabled
                        : null,
                      t,
                    )}
                  </span>
                )}
              </span>
            </Menu.Item>
          ))}
        <Menu.Item
          leftSection={<IconLink size={16} stroke={2} aria-hidden="true" />}
          disabled={!matchReady}
          onClick={() => commands.libraryInput('openMatch')}
        >
          <span className={menuItemStackClass}>
            <span>
              {actions.changeMatch
                ? t('Library::Change match…')
                : t('Library::Match…')}
            </span>
            {!matchReady && (
              <span className={menuItemReasonClass}>
                {formatReason(
                  typeof actions.matchGame === 'object' ? actions.matchGame.disabled : null,
                  t,
                )}
              </span>
            )}
          </span>
        </Menu.Item>

        <Menu.Divider />

        <Menu.Item
          leftSection={
            row.isFavorite ? (
              <IconStarFilled size={16} stroke={2} aria-hidden="true" />
            ) : (
              <IconStar size={16} stroke={2} aria-hidden="true" />
            )
          }
          disabled={!canFavorite}
          onClick={() =>
            commands.libraryInput({
              setFavorite: {
                key: row.key,
                favorite: !row.isFavorite,
                now: Math.floor(Date.now() / 1000),
              },
            })
          }
        >
          {row.isFavorite
            ? t('Library::Remove from favorites')
            : t('Library::Add to favorites')}
        </Menu.Item>
        <Menu.Item
          leftSection={
            row.hidden ? (
              <IconEye size={16} stroke={2} aria-hidden="true" />
            ) : (
              <IconEyeOff size={16} stroke={2} aria-hidden="true" />
            )
          }
          onClick={() =>
            commands.libraryInput({
              setHidden: { keys: [row.key], hidden: !row.hidden },
            })
          }
        >
          {row.hidden ? t('Library::Show in library') : t('Library::Hide from library')}
        </Menu.Item>

        <Menu.Divider />

        <Menu.Item
          leftSection={<IconTool size={16} stroke={2} aria-hidden="true" />}
          disabled={isRunning}
          onClick={() => commands.openTools(row.key)}
        >
          <span className={menuItemStackClass}>
            <span>
              {installedTools
                ? `${t('Library::Game tools…')} · ${installedTools}`
                : t('Library::Game tools…')}
            </span>
            {isRunning && (
              <span className={menuItemReasonClass}>{t('Library::Running')}</span>
            )}
          </span>
        </Menu.Item>
        {isDrmFree && (
          <>
            <Menu.Item
              leftSection={<IconSettings size={16} stroke={2} aria-hidden="true" />}
              onClick={() =>
                uiStore.setState((prev) => ({
                  ...prev,
                  localEditorEntryKey: row.key,
                }))
              }
            >
              {t('Library::Game settings…')}
            </Menu.Item>
            <Menu.Item
              leftSection={<IconShieldCheck size={16} stroke={2} aria-hidden="true" />}
              onClick={() => {
                uiStore.setState((prev) => ({
                  ...prev,
                  saveProtectionEntryKey: row.key,
                }));
                commands.loadLocalHistory(entryRegistrationId(row.key));
              }}
            >
              {t('Library::Local save protection')}
            </Menu.Item>
            <Menu.Item
              leftSection={<IconFolderOpen size={16} stroke={2} aria-hidden="true" />}
              onClick={() => commands.pickLocalExecutable(entryRegistrationId(row.key))}
            >
              {t('Library::Locate executable…')}
            </Menu.Item>
          </>
        )}
        {detail.achievementBackend && (
          <Menu.Item
            leftSection={<IconTrophy size={16} stroke={2} aria-hidden="true" />}
            onClick={onViewAchievements}
          >
            {t('Library::View achievements')}
          </Menu.Item>
        )}

        <Menu.Divider />

        <Menu.Item
          leftSection={<IconTrash size={16} stroke={2} aria-hidden="true" />}
          className={menuItemDangerClass}
          onClick={() =>
            uiStore.setState((prev) => ({
              ...prev,
              removeModalEntryKey: row.key,
            }))
          }
        >
          {t('Detail::Remove from library…')}
        </Menu.Item>
      </Menu.Dropdown>
    </MenuSurface>
  );
}
