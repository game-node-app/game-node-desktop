import React from 'react';
import {
  commands,
  useLibraryView,
  useLocalGames,
  usePortsView,
  useSnapshot,
  useTools,
  getMatchedGameId,
} from '../../api';
import { useUiStore, uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { useSessionClock } from '../../hooks/useSessionClock';
import { AchievementListModal } from './AchievementListModal';
import { DetailActions } from './detail/DetailActions';
import { DetailHero } from './detail/DetailHero';
import { DetailPills } from './detail/DetailPills';
import { DetailStats } from './detail/DetailStats';
import { GameNodeSection } from './detail/GameNodeSection';
import { LocalGameSections } from './detail/LocalGameSections';
import { MatchSection } from './detail/MatchSection';
import { AchievementsSection } from './detail/AchievementsSection';
import { MultiSelectionPane } from './detail/MultiSelectionPane';
import { OnThisPcSection } from './detail/OnThisPcSection';
import {
  entryRegistrationId,
  isDrmFreeKey,
  isInGameNodeLibrary,
  membershipStatus,
} from './detail/format';

const MIN_WIDTH = 260;
const MAX_WIDTH = 520;

export const DetailPane: React.FC = () => {
  const library = useLibraryView();
  const toolsView = useTools();
  const ports = usePortsView();
  const localGames = useLocalGames();
  const { data: launch } = useSnapshot((s) => s.launch);
  const { data: storeAccounts } = useSnapshot((s) => s.storeAccounts);
  const [achievementsOpen, setAchievementsOpen] = React.useState(false);
  const detailDragWidth = useUiStore((s) => s.detailDragWidth);
  const { t } = useI18n();

  const layout = library?.layout;
  const detail = library?.detail;
  const actions = library?.actions;
  const selectedCount = library?.selected?.length ?? 0;

  const currentWidth = detailDragWidth ?? layout?.detailWidth ?? 360;
  const clampedWidth = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, currentWidth));

  // Pointer drag for resizing
  const handlePointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    const startX = typeof e.clientX === 'number' && !isNaN(e.clientX) ? e.clientX : 500;
    const startW = clampedWidth;

    // Coalesce pointermove bursts into at most one store update per frame.
    let frame = 0;
    let pendingW = startW;

    const onPointerMove = (ev: MouseEvent | PointerEvent) => {
      const clientX = typeof ev.clientX === 'number' && !isNaN(ev.clientX) ? ev.clientX : startX;
      const delta = startX - clientX; // Dragging left expands pane
      pendingW = Math.max(MIN_WIDTH, Math.min(MAX_WIDTH, Math.round(startW + delta)));
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        if (uiStore.state.detailDragWidth !== pendingW) {
          uiStore.setState((prev) => ({ ...prev, detailDragWidth: pendingW }));
        }
      });
    };

    const onPointerUp = (ev: MouseEvent | PointerEvent) => {
      if (frame) {
        cancelAnimationFrame(frame);
        frame = 0;
      }
      window.removeEventListener('pointermove', onPointerMove as EventListener);
      window.removeEventListener('pointerup', onPointerUp as EventListener);
      window.removeEventListener('mousemove', onPointerMove as EventListener);
      window.removeEventListener('mouseup', onPointerUp as EventListener);
      const clientX = typeof ev.clientX === 'number' && !isNaN(ev.clientX) ? ev.clientX : startX;
      const finalDelta = startX - clientX;
      const finalW = Math.max(
        MIN_WIDTH,
        Math.min(MAX_WIDTH, Math.round(startW + finalDelta)),
      );
      uiStore.setState((prev) => ({ ...prev, detailDragWidth: null }));
      commands.libraryInput({ setDetailWidth: finalW });
    };

    window.addEventListener('pointermove', onPointerMove as EventListener);
    window.addEventListener('pointerup', onPointerUp as EventListener);
    window.addEventListener('mousemove', onPointerMove as EventListener);
    window.addEventListener('mouseup', onPointerUp as EventListener);
  };

  const handleHandleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowLeft') {
      const nextW = Math.min(MAX_WIDTH, clampedWidth + 16);
      commands.libraryInput({ setDetailWidth: nextW });
      e.preventDefault();
    } else if (e.key === 'ArrowRight') {
      const nextW = Math.max(MIN_WIDTH, clampedWidth - 16);
      commands.libraryInput({ setDetailWidth: nextW });
      e.preventDefault();
    }
  };

  const handleClose = () => {
    commands.libraryInput({ setDetailOpen: false });
  };

  const getLaunchMessage = (): string | null => {
    if (!launch) return null;
    if (launch.state === 'opening') {
      return launch.kind === 'play'
        ? t('Library::Opening the game in its store…')
        : t('Library::Opening the store to install…');
    }
    if (typeof launch.state === 'object' && 'failed' in launch.state) {
      switch (launch.state.failed) {
        case 'noHandler':
          return t('Library::The store application is not installed or cannot be opened.');
        case 'unsupported':
          return t('Library::This store does not support that action.');
        case 'failed':
        default:
          return t('Library::The link could not be opened.');
      }
    }
    return null;
  };

  const sharedLabel =
    library?.providers.find((provider) => provider.id === detail?.row.provider)?.sharedLabel ??
    null;
  // PC ports keys provenance by `drmfree:<id>`; the library's own manual entries are
  // `manual:<id>`. The host renders both from the same row.
  const portProvenance =
    detail?.row.key.startsWith('manual:')
      ? ports.provenance[`drmfree:${detail.row.key.slice('manual:'.length)}`]
      : undefined;

  const isDrmFree = detail ? isDrmFreeKey(detail.row.key) : false;
  const registrationId = detail ? entryRegistrationId(detail.row.key) : '';
  const registration = isDrmFree
    ? localGames.registrations.find(
        (candidate) =>
          candidate.id === registrationId ||
          candidate.id === detail?.row.key ||
          `manual:${candidate.id}` === detail?.row.key,
      )
    : undefined;
  const profile = isDrmFree
    ? localGames.profiles.filter((candidate) => candidate.installation === registrationId).at(-1)
    : undefined;
  const protectionActive = Boolean(profile && profile.support !== 'unsupported');
  const backups = isDrmFree
    ? localGames.versions.filter((version) => version.installation === registrationId)
    : [];
  const lastBackupAt = isDrmFree
    ? localGames.jobs
        .filter((job) => job.installation === registrationId)
        .sort((left, right) => right.readyAt - left.readyAt)[0]?.readyAt ?? null
    : null;
  const installPath = detail
    ? isDrmFree
      ? registration?.root ?? detail.installDir
      : detail.installDir
    : null;
  const gameId = detail ? getMatchedGameId(detail.row.status) : null;
  const inLibrary = detail ? isInGameNodeLibrary(detail.membership) : false;
  const inLibraryStatus = detail ? membershipStatus(detail.membership) ?? null : null;
  const isRunning = Boolean(detail && (detail.running || detail.row.running));
  const sessionStartedAt = detail?.row.sessionStartedAt ?? null;
  const sessionElapsedSeconds = useSessionClock(
    isRunning ? sessionStartedAt : null,
  );
  const tools = detail ? toolsView?.installedByEntry?.[detail.row.key] ?? [] : [];
  const providerAccount = storeAccounts.find(
    (account) => account.provider === detail?.row.provider,
  );
  const nowSeconds = Math.floor(Date.now() / 1000);

  return (
    <div
      data-testid="detail-pane"
      className="relative flex h-full shrink-0"
      // The declared width is the whole pane, resize handle included: the
      // handle is overlaid on the left edge instead of adding its own column.
      style={{ width: `${clampedWidth}px` }}
    >
      {/* Resizable Handle on left edge, overlapping the pane's first pixels */}
      <div
        data-testid="detail-resize-handle"
        role="slider"
        aria-label="Resize details pane"
        aria-valuenow={clampedWidth}
        aria-valuemin={MIN_WIDTH}
        aria-valuemax={MAX_WIDTH}
        tabIndex={0}
        onPointerDown={handlePointerDown}
        onKeyDown={handleHandleKeyDown}
        className="group absolute inset-y-0 left-0 z-10 flex w-[6px] cursor-col-resize touch-none select-none items-center justify-center bg-transparent outline-none"
      >
        <div className="h-full w-px bg-border-subtle transition-colors group-hover:bg-brand group-focus-visible:bg-brand" />
      </div>

      {/* Pane Content */}
      <div className="flex h-full min-w-0 flex-1 flex-col overflow-hidden border-l border-border-subtle bg-secondary">
        {!detail && selectedCount > 1 ? (
          <MultiSelectionPane
            selected={library?.selected ?? []}
            rows={library?.rows ?? []}
            actions={actions}
            collections={library?.collections ?? []}
          />
        ) : !detail ? (
          <div
            data-testid="detail-empty-message"
            className="flex flex-1 items-center justify-center p-[24px] text-center text-sm text-text-dimmed"
          >
            {t('Library::Select a game to see its details.')}
          </div>
        ) : (
          <>
            <DetailHero
              detail={detail}
              sharedLabel={sharedLabel}
              onClose={handleClose}
            />

            <div className="flex flex-1 flex-col gap-[18px] overflow-y-auto p-[16px]">
              <DetailActions
                detail={detail}
                actions={actions}
                installPath={installPath}
                isRunning={isRunning}
                sessionElapsedSeconds={sessionElapsedSeconds}
                isDrmFree={isDrmFree}
                inLibrary={inLibrary}
                gameId={gameId}
                tools={tools}
                onViewAchievements={() => setAchievementsOpen(true)}
              />

              {launch && (
                <div
                  data-testid="detail-launch-status"
                  className={`flex flex-col gap-1 rounded-control px-3 py-2 ${
                    launch.state === 'opening' ? 'bg-accent-subtle' : 'bg-danger-subtle'
                  }`}
                >
                  <span className="text-xs text-text-default">{getLaunchMessage()}</span>
                  {launch.state !== 'opening' && (
                    <button
                      type="button"
                      data-testid="detail-dismiss-launch-btn"
                      onClick={() => commands.dismissLaunch()}
                      className="self-start rounded-xs border border-border-subtle bg-transparent px-2 py-1 text-xs text-text-default transition-colors hover:bg-surface"
                    >
                      {t('Library::Dismiss')}
                    </button>
                  )}
                </div>
              )}

              <DetailPills
                detail={detail}
                isDrmFree={isDrmFree}
                protectionActive={protectionActive}
                inLibrary={inLibrary}
                inLibraryStatus={inLibraryStatus}
                isFamilyShared={detail.row.shared && !detail.row.owned}
              />

              <DetailStats
                detail={detail}
                isDrmFree={isDrmFree}
                protectionActive={protectionActive}
                backupsCount={backups.length}
                lastBackupAt={lastBackupAt}
                sessionStartedAt={sessionStartedAt}
                sessionElapsedSeconds={sessionElapsedSeconds}
                nowSeconds={nowSeconds}
              />

              {detail.row.shared && !detail.row.owned && (
                <div
                  data-testid="detail-family-note"
                  className="flex items-start gap-2.5 rounded-lg border border-border-subtle bg-surface p-3"
                >
                  <span className="mt-0.5 text-text-secondary" aria-hidden="true">
                    i
                  </span>
                  <p className="m-0 text-xs leading-[17px] text-text-secondary">
                    {t(
                      'Detail::This game comes from Steam Family. Playtime and achievements are your own; access may pause while the owner is playing.',
                    )}
                  </p>
                </div>
              )}

              <GameNodeSection
                detail={detail}
                actions={actions}
                membership={detail.membership}
                collections={library?.collections ?? []}
                gameId={gameId}
              />

              <MatchSection detail={detail} actions={actions} />

              {detail.achievementBackend && (
                <AchievementsSection
                  detail={detail}
                  providerAccount={providerAccount}
                  onOpen={() => setAchievementsOpen(true)}
                />
              )}

              <OnThisPcSection
                detail={detail}
                isDrmFree={isDrmFree}
                installPath={installPath}
                tools={tools}
                isRunning={isRunning}
                portProvenance={portProvenance ?? null}
              />

              {isDrmFree ? (
                <LocalGameSections
                  detail={detail}
                  registration={registration}
                  profile={profile}
                  tools={tools}
                  isRunning={isRunning}
                  protectionActive={protectionActive}
                />
              ) : (
                <button
                  type="button"
                  data-testid="detail-remove-btn"
                  onClick={() =>
                    uiStore.setState((prev) => ({
                      ...prev,
                      removeModalEntryKey: detail.row.key,
                    }))
                  }
                  className="flex items-center gap-1.5 self-start rounded-xs text-[13px] font-medium text-danger transition-colors hover:text-danger/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger"
                >
                  {t('Detail::Remove from library…')}
                </button>
              )}
            </div>
          </>
        )}
      </div>

      {detail?.achievementBackend && (
        <AchievementListModal
          opened={achievementsOpen}
          onClose={() => setAchievementsOpen(false)}
          entryKey={detail.row.key}
          gameTitle={detail.row.title}
          providerName={detail.row.providerName}
        />
      )}
    </div>
  );
};
