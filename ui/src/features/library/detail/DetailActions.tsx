import {
  IconDownload,
  IconFolderOpen,
  IconPlayerPlayFilled,
  IconStar,
  IconStarFilled,
} from '@tabler/icons-react';
import {
  commands,
  type Actions,
  type DetailView,
  type GameToolView,
} from '../../../api';
import { Button, IconButton } from '../../../components/ui';
import { formatSessionTime } from '../../../hooks/useSessionClock';
import { useI18n } from '../../../i18n';
import { formatI18n, formatReason } from '../formatters';
import { MoreActionsMenu } from './MoreActionsMenu';

export interface DetailActionsProps {
  detail: DetailView;
  actions: Actions | undefined;
  installPath: string | null;
  isRunning: boolean;
  /** Live seconds since the running execution began; null when unknown. */
  sessionElapsedSeconds?: number | null;
  isDrmFree: boolean;
  inLibrary: boolean;
  gameId: number | null;
  tools: GameToolView[];
  onViewAchievements: () => void;
}

/**
 * Detail hero actions: Play or Install, favorite, open install folder and the more-actions menu.
 * A running game replaces Play with a non-interactive running indicator.
 */
export function DetailActions({
  detail,
  actions,
  installPath,
  isRunning,
  sessionElapsedSeconds = null,
  isDrmFree,
  inLibrary,
  gameId,
  tools,
  onViewAchievements,
}: DetailActionsProps) {
  const { t } = useI18n();
  const row = detail.row;
  const playReady = actions?.play === 'available';
  const installAvailable = actions?.install === 'available';
  const installHasRule =
    actions?.install === 'available' ||
    (typeof actions?.install === 'object' &&
      actions.install.disabled !== 'noInstallRule');
  const installPrimary = !row.installed && installHasRule;
  const playReason =
    actions && actions.play !== 'available'
      ? formatReason(actions.play.disabled, t)
      : '';
  const installReason =
    actions && actions.install !== 'available'
      ? formatReason(actions.install.disabled, t)
      : '';

  return (
    <div className="flex flex-col gap-[8px]">
      <div className="flex items-center gap-[8px]">
        {isRunning ? (
          <div
            data-testid="detail-running-indicator"
            className="flex h-[36px] min-w-0 flex-1 items-center justify-center gap-[6px] rounded-control border border-success bg-success-subtle px-[12px] text-sm font-semibold text-text-primary"
          >
            <IconPlayerPlayFilled size={16} aria-hidden="true" className="text-success" />
            <span className="truncate">
              {sessionElapsedSeconds == null
                ? t('Library::Running')
                : formatI18n(
                    t('Library::Running · {}'),
                    formatSessionTime(sessionElapsedSeconds),
                  )}
            </span>
          </div>
        ) : installPrimary ? (
          <>
            <Button
              variant="primary"
              fullWidth
              data-testid="detail-install-btn"
              disabled={!installAvailable}
              leftIcon={IconDownload}
              onClick={() => commands.launchSelected('install')}
            >
              {t('Detail::Install')}
            </Button>
            {playReady && (
              <IconButton
                icon={IconPlayerPlayFilled}
                label={t('Detail::Play')}
                variant="outlined"
                data-testid="detail-play-btn"
                onClick={() => commands.launchSelected('play')}
              />
            )}
          </>
        ) : (
          <Button
            variant="primary"
            fullWidth
            data-testid="detail-play-btn"
            disabled={!playReady}
            leftIcon={IconPlayerPlayFilled}
            onClick={() => commands.launchSelected('play')}
          >
            {t('Detail::Play')}
          </Button>
        )}

        <IconButton
          icon={row.isFavorite ? IconStarFilled : IconStar}
          label={
            row.isFavorite
              ? t('Library::Remove from favorites')
              : t('Library::Add to favorites')
          }
          variant="outlined"
          aria-pressed={row.isFavorite}
          disabled={gameId == null}
          data-testid="detail-favorite-btn"
          className={row.isFavorite ? '[&_svg]:text-warning' : undefined}
          onClick={() =>
            commands.libraryInput({
              setFavorite: {
                key: row.key,
                favorite: !row.isFavorite,
                now: Math.floor(Date.now() / 1000),
              },
            })
          }
        />

        {row.installed && installPath && (
          <IconButton
            icon={IconFolderOpen}
            label={t('Library::Open install folder')}
            variant="outlined"
            data-testid="detail-open-folder-btn"
            onClick={() => void commands.openInstallFolder(installPath)}
          />
        )}

        <MoreActionsMenu
          detail={detail}
          actions={
            actions ?? {
              selected: 1,
              play: { disabled: 'nothingSelected' },
              install: { disabled: 'nothingSelected' },
              matchGame: { disabled: 'nothingSelected' },
              addToGamenode: { disabled: 'nothingSelected' },
              changeMatch: false,
            }
          }
          installPath={installPath}
          isRunning={isRunning}
          isDrmFree={isDrmFree}
          inLibrary={inLibrary}
          gameId={gameId}
          tools={tools}
          onViewAchievements={onViewAchievements}
        />
      </div>

      {!isRunning && !installPrimary && !playReady && playReason && (
        <div data-testid="detail-play-reason" className="text-xs text-warning">
          {playReason}
        </div>
      )}
      {!isRunning && installPrimary && !installAvailable && installReason && (
        <div data-testid="detail-install-reason" className="text-xs text-warning">
          {installReason}
        </div>
      )}
      {gameId == null && (
        <div data-testid="detail-favorite-reason" className="text-xs text-warning">
          {t('Library::Match the game first to use favorites.')}
        </div>
      )}
    </div>
  );
}
