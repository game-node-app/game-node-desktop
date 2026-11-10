import React from 'react';
import {
  IconChevronRight,
  IconDownload,
  IconPlayerPlay,
  IconStar,
  IconStarFilled,
  IconUsersGroup,
  IconX,
} from '@tabler/icons-react';
import {
  commands,
  useLibraryView,
  useSnapshot,
  useTools,
  usePortsView,
  artworkAssetUrl,
  getMatchedGameId,
  type Membership,
} from '../../api';
import { useUiStore, uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { CoverImage } from './CoverImage';
import { AchievementListModal } from './AchievementListModal';
import {
  formatI18n,
  formatReason,
  formatStatus,
  formatDuration,
  formatMatchStatus,
} from './formatters';

const secondaryActionBase =
  'w-full rounded-xs border border-border-subtle bg-transparent px-2.5 py-compact text-left text-sm text-text-default transition-colors';
const reasonClass = 'mt-1 text-xs text-warning';

export const DetailPane: React.FC = () => {
  const library = useLibraryView();
  const tools = useTools();
  const ports = usePortsView();
  const { data: launch } = useSnapshot((s) => s.launch);
  const { data: storeAccounts } = useSnapshot((s) => s.storeAccounts);
  const [achievementsOpen, setAchievementsOpen] = React.useState(false);
  const detailDragWidth = useUiStore((s) => s.detailDragWidth);
  const { t, locale } = useI18n();

  const layout = library?.layout;
  const detail = library?.detail;
  // PC ports keys provenance by `drmfree:<id>`; the library's own manual entries are
  // `manual:<id>`. The host renders both from the same row.
  const portProvenance =
    detail?.row.key.startsWith('manual:')
      ? ports.provenance[`drmfree:${detail.row.key.slice('manual:'.length)}`]
      : undefined;
  const actions = library?.actions;
  const selectedCount = library?.selected?.length ?? 0;

  const currentWidth = detailDragWidth ?? layout?.detailWidth ?? 320;
  const clampedWidth = Math.max(260, Math.min(520, currentWidth));

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
      pendingW = Math.max(260, Math.min(520, Math.round(startW + delta)));
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
      const finalW = Math.max(260, Math.min(520, Math.round(startW + finalDelta)));
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
      const nextW = Math.min(520, clampedWidth + 16);
      commands.libraryInput({ setDetailWidth: nextW });
      e.preventDefault();
    } else if (e.key === 'ArrowRight') {
      const nextW = Math.max(260, clampedWidth - 16);
      commands.libraryInput({ setDetailWidth: nextW });
      e.preventDefault();
    }
  };

  const handleClose = () => {
    commands.libraryInput({ setDetailOpen: false });
  };

  const formatMembershipStanding = (membership?: Membership | null): string => {
    if (!membership || membership === 'notChecked') return '';
    if (membership === 'checking') {
      return t('Library::Checking your GameNode library…');
    }
    if (membership === 'unavailable') {
      return t('Library::Could not check your GameNode library');
    }
    if (membership === 'notInLibrary') {
      return t('Library::Not in your GameNode library');
    }
    if (typeof membership === 'object' && 'inLibrary' in membership) {
      if (membership.inLibrary != null) {
        return formatI18n(
          t('Library::In your GameNode library as {}'),
          formatStatus(membership.inLibrary, t),
        );
      }
      return t('Library::In your GameNode library');
    }
    return '';
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
        case 'missingHandler':
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

  const providerAccount = storeAccounts.find(
    (account) => account.provider === detail?.row.provider,
  );
  const achievementConnected = providerAccount?.state === 'connected';
  const achievementSummary = detail?.achievements;

  const formatAchievementTime = (at: number | null | undefined): string | null => {
    if (at == null) return null;
    return new Intl.DateTimeFormat(locale, {
      dateStyle: 'medium',
      timeStyle: 'short',
    }).format(new Date(at * 1000));
  };

  const achievementUpdatedAt = formatAchievementTime(
    achievementSummary?.lastSuccessAt,
  );
  const isPlayAvailable = actions?.play === 'available';
  const isInstallAvailable = actions?.install === 'available';
  const isRunning = detail?.running || detail?.row.running;
  const favoriteGameId = detail ? getMatchedGameId(detail.row.status) : null;

  return (
    <div data-testid="detail-pane" className="relative flex h-full shrink-0">
      {/* Resizable Handle on left edge */}
      <div
        data-testid="detail-resize-handle"
        role="slider"
        aria-label="Resize details pane"
        aria-valuenow={clampedWidth}
        aria-valuemin={260}
        aria-valuemax={520}
        tabIndex={0}
        onPointerDown={handlePointerDown}
        onKeyDown={handleHandleKeyDown}
        className="group relative flex h-full w-2 cursor-col-resize select-none items-center justify-center bg-transparent outline-none"
      >
        <div className="h-full w-px bg-border-subtle transition-colors group-hover:bg-brand group-focus-visible:bg-brand" />
      </div>

      {/* Pane Content */}
      <div
        className="flex h-full flex-col overflow-hidden border-l border-border-subtle bg-secondary"
        style={{ width: `${clampedWidth}px` }}
      >
        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-border-subtle px-md py-sm">
          <span className="text-lg font-semibold text-text-default">
            {t('Library::Details')}
          </span>
          <button
            type="button"
            data-testid="detail-close-btn"
            aria-label="Close"
            onClick={handleClose}
            className="rounded-sm p-1 text-text-default transition-colors hover:bg-surface"
          >
            <IconX size={18} stroke={1.75} aria-hidden />
          </button>
        </div>

        {/* Scrollable Body */}
        <div className="flex flex-1 flex-col gap-md overflow-y-auto p-md">
          {/* Multiple selected state */}
          {!detail && selectedCount > 1 && (
            <div data-testid="detail-multiple-selected">
              <h3 className="mb-sm text-md text-text-default">
                {formatI18n(t('Library::{} games selected'), selectedCount)}
              </h3>
              <div className="rounded-sm bg-raised p-sm">
                <button
                  type="button"
                  data-testid="detail-add-multiple-btn"
                  disabled={actions?.addToGamenode !== 'available'}
                  onClick={() => commands.libraryInput('openAdd')}
                  className={`w-full rounded-sm border-0 bg-brand px-sm py-compact text-on-primary transition-colors ${
                    actions?.addToGamenode === 'available'
                      ? 'cursor-pointer opacity-100 hover:bg-brand-6'
                      : 'cursor-default opacity-50'
                  }`}
                >
                  {t('Library::Add to GameNode…')}
                </button>
                {actions?.addToGamenode !== 'available' && (
                  <div
                    data-testid="detail-add-multiple-reason"
                    className={`${reasonClass} mt-1.5`}
                  >
                    {formatReason(actions?.addToGamenode.disabled, t)}
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Nothing selected */}
          {!detail && selectedCount <= 1 && (
            <div
              data-testid="detail-empty-message"
              className="pt-xl text-center text-sm text-text-dimmed"
            >
              {t('Library::Select a game to see its details.')}
            </div>
          )}

          {/* Valid detail view */}
          {detail && (
            <>
              {/* Artwork / Hero */}
              <div
                data-testid="detail-artwork-container"
                className="h-[180px] w-full shrink-0 overflow-hidden rounded-sm bg-surface"
              >
                {detail.hasArtwork && getMatchedGameId(detail.row.status) != null ? (
                  <img
                    data-testid="detail-artwork-img"
                    src={artworkAssetUrl(getMatchedGameId(detail.row.status))!}
                    alt={detail.row.title}
                    loading="lazy"
                    decoding="async"
                    className="block h-full w-full object-cover"
                  />
                ) : (
                  <CoverImage
                    coverFile={detail.row.coverFile}
                    title={detail.row.title}
                    detectedName={detail.row.detectedName}
                  />
                )}
              </div>

              {/* Title & Metadata */}
              <div>
                {(detail.running || detail.row.running) && (
                  <div
                    data-testid="detail-running-badge"
                    className="mb-1 text-xs font-semibold text-success"
                  >
                    {t('Library::Running')}
                  </div>
                )}
                <h2
                  data-testid="detail-title"
                  className="text-lg font-semibold leading-[1.3] text-text-default"
                >
                  {detail.row.title}
                </h2>
                <div className="mt-1 flex flex-wrap items-center gap-1.5 text-xs text-text-dimmed">
                  <span>
                    {detail.releaseYear ? `${detail.releaseYear} · ` : ''}
                    {detail.row.providerName}
                  </span>
                  {detail.row.familyShared && !detail.row.owned && (
                    <span
                      data-testid="detail-family-label"
                      className="inline-flex items-center gap-1 rounded-xs bg-accent-subtle px-1.5 py-px text-[11px] font-medium text-brand"
                    >
                      <IconUsersGroup size={12} stroke={1.75} aria-hidden />
                      {t('Library::Steam Family')}
                    </span>
                  )}
                </div>
              </div>

              {/* Favorite */}
              <div>
                <button
                  type="button"
                  data-testid="detail-favorite-btn"
                  aria-pressed={detail.row.isFavorite}
                  disabled={favoriteGameId == null}
                  onClick={() =>
                    commands.libraryInput({
                      setFavorite: {
                        key: detail.row.key,
                        favorite: !detail.row.isFavorite,
                        now: Math.floor(Date.now() / 1000),
                      },
                    })
                  }
                  className={`flex w-full items-center justify-center gap-1.5 rounded-sm border border-border-subtle bg-transparent px-md py-compact text-sm font-medium transition-colors ${
                    favoriteGameId == null
                      ? 'cursor-default text-text-dimmed opacity-45'
                      : detail.row.isFavorite
                        ? 'cursor-pointer text-warning hover:bg-surface'
                        : 'cursor-pointer text-text-default hover:bg-surface'
                  }`}
                >
                  {detail.row.isFavorite ? (
                    <IconStarFilled size={16} aria-hidden />
                  ) : (
                    <IconStar size={16} stroke={1.75} aria-hidden />
                  )}
                  {detail.row.isFavorite
                    ? t('Library::Remove from favorites')
                    : t('Library::Add to favorites')}
                </button>
                {favoriteGameId == null && (
                  <div
                    data-testid="detail-favorite-reason"
                    className={`${reasonClass} mt-1`}
                  >
                    {t('Library::Match the game first to use favorites.')}
                  </div>
                )}
              </div>

              {/* Launch Status Banner */}
              {launch && (
                <div
                  data-testid="detail-launch-status"
                  className={`flex flex-col gap-compact rounded-sm px-sm py-xs ${
                    launch.state === 'opening' ? 'bg-accent-subtle' : 'bg-danger-subtle'
                  }`}
                >
                  <span className="text-xs text-text-default">
                    {getLaunchMessage()}
                  </span>
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

              {/* Primary Actions (Play, Install) */}
              <div className="flex flex-col gap-compact">
                {/* Play Button */}
                <div>
                  <button
                    type="button"
                    data-testid="detail-play-btn"
                    disabled={!isPlayAvailable}
                    onClick={() => commands.launchSelected('play')}
                    className={`flex w-full items-center justify-center gap-1.5 rounded-sm border-0 bg-brand px-md py-xs text-sm font-semibold text-on-primary transition-colors ${
                      isPlayAvailable
                        ? 'cursor-pointer opacity-100 hover:bg-brand-6'
                        : 'cursor-default opacity-45'
                    }`}
                  >
                    <IconPlayerPlay size={16} stroke={1.75} aria-hidden />
                    {t('Library::Play')}
                  </button>
                  {actions?.play !== 'available' && (
                    <div data-testid="detail-play-reason" className={`${reasonClass} mt-1`}>
                      {formatReason(actions?.play.disabled, t)}
                    </div>
                  )}
                </div>

                {/* Install Button */}
                {actions?.install &&
                  (typeof actions.install !== 'object' ||
                    actions.install.disabled !== 'noInstallRule') && (
                    <div>
                      <button
                        type="button"
                        data-testid="detail-install-btn"
                        disabled={!isInstallAvailable}
                        onClick={() => commands.launchSelected('install')}
                        className={`flex w-full items-center justify-center gap-1.5 rounded-sm border border-border-subtle bg-transparent px-md py-xs text-sm font-medium text-text-default transition-colors ${
                          isInstallAvailable
                            ? 'cursor-pointer opacity-100 hover:bg-surface'
                            : 'cursor-default opacity-45'
                        }`}
                      >
                        <IconDownload size={16} stroke={1.75} aria-hidden />
                        {t('Library::Install')}
                      </button>
                      {actions.install !== 'available' && (
                        <div
                          data-testid="detail-install-reason"
                          className={`${reasonClass} mt-1`}
                        >
                          {formatReason(actions.install.disabled, t)}
                        </div>
                      )}
                    </div>
                  )}
              </div>

              {/* Playtime Row */}
              <div className="flex items-center justify-between rounded-sm bg-raised px-sm py-compact text-xs">
                <span className="text-text-dimmed">
                  {t('Library::Playtime')}
                </span>
                <span
                  data-testid="detail-playtime-val"
                  className="font-medium text-text-default"
                >
                  {formatDuration(detail.row.playtime?.seconds, t)}
                </span>
                <span className="text-[11px] text-text-dimmed">
                  {detail.row.playtime?.source === 'store'
                    ? formatI18n(t('Library::Imported from {}'), detail.row.providerName)
                    : t('Library::Internal counter')}
                </span>
              </div>

              {/* Achievements */}
              {detail.achievementBackend && (
                <div data-testid="detail-achievements" className="flex flex-col gap-1.5">
                  <span className="text-xs font-semibold uppercase tracking-[0.5px] text-text-dimmed">
                    {t('Library::Achievements')}
                  </span>
                  <div className="flex flex-col gap-compact rounded-sm bg-raised px-sm py-compact">
                    {achievementSummary?.state === 'ready' ? (
                      <>
                        <div className="flex items-center justify-between text-sm">
                          <span className="text-text-default">
                            {formatI18n(
                              t('Library::{} of {} unlocked'),
                              achievementSummary.unlocked,
                              achievementSummary.total,
                            )}
                          </span>
                          <span className="text-xs text-text-dimmed">
                            {achievementSummary.total > 0
                              ? `${Math.round(
                                  (achievementSummary.unlocked / achievementSummary.total) * 100,
                                )}%`
                              : '—'}
                          </span>
                        </div>
                        <div
                          data-testid="detail-achievement-progress"
                          role="progressbar"
                          aria-label={t('Library::Achievement progress')}
                          aria-valuemin={0}
                          aria-valuemax={achievementSummary.total}
                          aria-valuenow={achievementSummary.unlocked}
                          className="h-1.5 w-full overflow-hidden rounded-full bg-surface"
                        >
                          <div
                            className="h-full rounded-full bg-brand"
                            style={{
                              width: `${
                                achievementSummary.total > 0
                                  ? (achievementSummary.unlocked / achievementSummary.total) * 100
                                  : 0
                              }%`,
                            }}
                          />
                        </div>
                        {achievementSummary.latestUnlock && (
                          <div
                            data-testid="detail-achievement-latest"
                            className="text-xs text-text-dimmed"
                          >
                            {t('Library::Latest unlock')}:{' '}
                            <span className="text-text-default">
                              {achievementSummary.latestUnlock.name}
                            </span>
                            {formatAchievementTime(
                              achievementSummary.latestUnlock.unlockedAt,
                            ) && (
                              <span>
                                {' '}
                                ·{' '}
                                {formatAchievementTime(
                                  achievementSummary.latestUnlock.unlockedAt,
                                )}
                              </span>
                            )}
                          </div>
                        )}
                        {achievementSummary.failure && (
                          <div
                            data-testid="detail-achievement-error"
                            className="text-xs text-warning"
                          >
                            {achievementSummary.failure === 'privacy'
                              ? t(
                                  'Library::Steam privacy settings prevent reading these achievements.',
                                )
                              : t(
                                  'Library::Achievements could not be updated. Showing the last saved data.',
                                )}
                          </div>
                        )}
                        {achievementUpdatedAt && (
                          <div
                            data-testid="detail-achievement-updated"
                            className="text-[11px] text-text-dimmed"
                          >
                            {formatI18n(
                              t('Library::Updated {}'),
                              achievementUpdatedAt,
                            )}
                          </div>
                        )}
                        <button
                          type="button"
                          data-testid="detail-achievements-open"
                          onClick={() => setAchievementsOpen(true)}
                          className="mt-1 flex items-center justify-between rounded-xs border border-border-subtle bg-transparent px-2 py-1 text-left text-xs text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                        >
                          {t('Library::View all achievements')}
                          <IconChevronRight size={14} stroke={1.75} aria-hidden />
                        </button>
                      </>
                    ) : achievementSummary?.state === 'none' ? (
                      <div
                        data-testid="detail-achievements-none"
                        className="text-sm text-text-dimmed"
                      >
                        {t('Library::This game has no achievements.')}
                      </div>
                    ) : achievementSummary?.state === 'failed' ? (
                      <div
                        data-testid="detail-achievements-failed"
                        className="text-sm text-warning"
                      >
                        {achievementSummary.failure === 'privacy'
                          ? t(
                              'Library::Steam privacy settings prevent reading these achievements.',
                            )
                          : t('Library::Achievements could not be loaded.')}
                      </div>
                    ) : achievementConnected ? (
                      <div
                        data-testid="detail-achievements-loading"
                        className="text-sm text-text-dimmed"
                      >
                        {t('Library::Loading achievements…')}
                      </div>
                    ) : providerAccount?.state === 'expired' ? (
                      <div
                        data-testid="detail-achievements-expired"
                        className="text-sm text-text-dimmed"
                      >
                        {t('Library::Sign in to Steam again to see achievements.')}
                      </div>
                    ) : (
                      <div
                        data-testid="detail-achievements-not-connected"
                        className="text-sm text-text-dimmed"
                      >
                        {t('Library::Connect Steam to see achievements.')}
                      </div>
                    )}
                  </div>
                </div>
              )}

              {/* GameNode Card */}
              <div className="flex flex-col gap-1.5">
                <span className="text-xs font-semibold uppercase tracking-[0.5px] text-text-dimmed">
                  {t('Library::GameNode')}
                </span>

                <div className="flex flex-col gap-compact rounded-sm bg-raised px-sm py-compact">
                  {/* Match / Change Match */}
                  <div>
                    <button
                      type="button"
                      data-testid="detail-match-btn"
                      disabled={actions?.matchGame !== 'available'}
                      onClick={() => commands.libraryInput('openMatch')}
                      className={`${secondaryActionBase} ${
                        actions?.matchGame === 'available'
                          ? 'cursor-pointer opacity-100 hover:bg-surface'
                          : 'cursor-default opacity-45'
                      }`}
                    >
                      {actions?.changeMatch
                        ? t('Library::Change match…')
                        : t('Library::Match…')}
                    </button>
                    {actions?.matchGame !== 'available' && (
                      <div data-testid="detail-match-reason" className={reasonClass}>
                        {formatReason(actions?.matchGame.disabled, t)}
                      </div>
                    )}
                  </div>

                  {/* Add to GameNode / Edit in GameNode */}
                  <div>
                    <button
                      type="button"
                      data-testid="detail-add-btn"
                      disabled={actions?.addToGamenode !== 'available'}
                      onClick={() => commands.libraryInput('openAdd')}
                      className={`${secondaryActionBase} ${
                        actions?.addToGamenode === 'available'
                          ? 'cursor-pointer opacity-100 hover:bg-surface'
                          : 'cursor-default opacity-45'
                      }`}
                    >
                      {typeof detail.membership === 'object' &&
                      'inLibrary' in detail.membership
                        ? t('Library::Edit in GameNode…')
                        : t('Library::Add to GameNode…')}
                    </button>
                    {actions?.addToGamenode !== 'available' && (
                      <div data-testid="detail-add-reason" className={reasonClass}>
                        {formatReason(actions?.addToGamenode.disabled, t)}
                      </div>
                    )}
                  </div>

                  {/* Check again if membership unavailable */}
                  {actions?.addToGamenode !== 'available' &&
                    actions?.addToGamenode.disabled === 'membershipUnavailable' && (
                      <button
                        type="button"
                        data-testid="detail-refresh-membership-btn"
                        onClick={() => commands.libraryInput('refreshMembership')}
                        className="self-start rounded-xs border border-border-subtle bg-transparent px-2 py-1 text-xs text-text-default transition-colors hover:bg-surface"
                      >
                        {t('Library::Check again')}
                      </button>
                    )}
                </div>
              </div>

              {/* Manage Card */}
              <div className="flex flex-col gap-1.5">
                <span className="text-xs font-semibold uppercase tracking-[0.5px] text-text-dimmed">
                  {t('Library::Manage')}
                </span>

                <div className="flex flex-col gap-compact rounded-sm bg-raised px-sm py-compact">
                  {/* Game Tools */}
                  <div>
                    <button
                      type="button"
                      data-testid="detail-tools-btn"
                      disabled={isRunning}
                      onClick={() => commands.openTools(detail.row.key)}
                      className={`${secondaryActionBase} ${
                        !isRunning
                          ? 'cursor-pointer opacity-100 hover:bg-surface'
                          : 'cursor-default opacity-45'
                      }`}
                    >
                      {t('Library::Game tools…')}
                    </button>
                    {/* Tools installed summary */}
                    {tools?.installedByEntry?.[detail.row.key]?.length ? (
                      <div
                        data-testid="detail-tools-summary"
                        className="mt-1 text-xs text-text-dimmed"
                      >
                        {tools.installedByEntry[detail.row.key]
                          .map((tItem) => tItem.name)
                          .join(', ')}
                      </div>
                    ) : null}
                  </div>

                  {/* Manual entry actions */}
                  {detail.row.key.startsWith('manual:') && (
                    <>
                      {/* Game settings / Edit game */}
                      <button
                        type="button"
                        data-testid="detail-edit-game-btn"
                        onClick={() =>
                          uiStore.setState((prev) => ({
                            ...prev,
                            localEditorEntryKey: detail.row.key,
                          }))
                        }
                        className={`${secondaryActionBase} cursor-pointer hover:bg-surface`}
                      >
                        {locale === 'pt-BR'
                          ? t('DRM-free::Game settings')
                          : 'Edit game'}
                      </button>

                      {/* Local save protection */}
                      <button
                        type="button"
                        data-testid="detail-save-protection-btn"
                        onClick={() => {
                          uiStore.setState((prev) => ({
                            ...prev,
                            saveProtectionEntryKey: detail.row.key,
                          }));
                          commands.loadLocalHistory(detail.row.key.replace('manual:', ''));
                        }}
                        className={`${secondaryActionBase} cursor-pointer hover:bg-surface`}
                      >
                        {locale === 'pt-BR'
                          ? t('DRM-free::Local save protection')
                          : 'Save protection'}
                      </button>

                      {/* Locate executable */}
                      <button
                        type="button"
                        data-testid="detail-locate-btn"
                        onClick={() =>
                          commands.pickLocalExecutable(
                            detail.row.key.replace('manual:', ''),
                          )
                        }
                        className={`${secondaryActionBase} cursor-pointer hover:bg-surface`}
                      >
                        {t('DRM-free::Locate executable')}
                      </button>
                    </>
                  )}
                </div>
              </div>

              {/* Information Card */}
              <div className="flex flex-col gap-2.5 rounded-sm bg-raised p-sm text-xs">
                {/* Detected title */}
                <div>
                  <div className="text-text-dimmed">
                    {t('Library::Detected title')}
                  </div>
                  <div
                    data-testid="detail-detected-title"
                    className="font-medium text-text-default"
                  >
                    {detail.row.detectedName}
                  </div>
                </div>

                {/* GameNode game */}
                <div>
                  <div className="text-text-dimmed">
                    {t('Library::GameNode game')}
                  </div>
                  <div
                    data-testid="detail-gamenode-title"
                    className="font-medium text-text-default"
                  >
                    {detail.row.matchedName || '—'}
                  </div>
                </div>

                {/* Install folder, or the not-installed statement for catalog-only games */}
                {detail.row.installed ? (
                  <div>
                    <div className="text-text-dimmed">
                      {t('Library::Install folder')}
                    </div>
                    <div
                      data-testid="detail-install-dir"
                      className="break-all text-text-default"
                    >
                      {detail.installDir}
                    </div>
                  </div>
                ) : (
                  <div
                    data-testid="detail-not-installed"
                    className="text-text-default"
                  >
                    {t('Library::Not installed on this PC')}
                  </div>
                )}

                {/* PC ports provenance */}
                {portProvenance && (
                  <div data-testid="detail-port-provenance">
                    <div className="text-text-dimmed">
                      {t('Library::Installed by PC ports')}
                    </div>
                    <div className="font-medium text-text-default">
                      {formatI18n(
                        t('Library::{} version {}'),
                        portProvenance.portName,
                        portProvenance.installedVersion,
                      )}
                    </div>
                    <button
                      type="button"
                      data-testid="detail-port-link"
                      onClick={() =>
                        uiStore.setState((prev) => ({
                          ...prev,
                          activeDestination: 'ports',
                          portsFocusCatalogId: portProvenance.catalogId,
                        }))
                      }
                      className="mt-1 rounded-xs text-left text-xs text-brand transition-colors hover:text-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                    >
                      {t('Library::Open in PC ports')}
                    </button>
                  </div>
                )}

                {/* Match */}
                <div>
                  <div className="text-text-dimmed">
                    {t('Library::Match')}
                  </div>
                  <div
                    data-testid="detail-match-status"
                    className="text-text-default"
                  >
                    {formatMatchStatus(detail.row.status, t)}
                  </div>
                </div>

                {/* In your GameNode library */}
                <div>
                  <div className="text-text-dimmed">
                    {t('Library::In your GameNode library')}
                  </div>
                  <div
                    data-testid="detail-membership-standing"
                    className="text-text-default"
                  >
                    {formatMembershipStanding(detail.membership)}
                  </div>
                </div>
              </div>

              {/* Remove from library — available for manual and store entries */}
              <div className="rounded-sm bg-raised px-sm py-compact">
                <button
                  type="button"
                  data-testid="detail-remove-btn"
                  onClick={() =>
                    uiStore.setState((prev) => ({
                      ...prev,
                      removeModalEntryKey: detail.row.key,
                    }))
                  }
                  className="w-full rounded-xs border border-danger bg-transparent px-2.5 py-compact text-left text-sm text-danger transition-colors hover:bg-danger-subtle"
                >
                  {locale === 'pt-BR'
                    ? t('DRM-free::Remove from library')
                    : 'Remove'}
                </button>
              </div>
            </>
          )}
        </div>
      </div>
      {detail?.achievementBackend && (
        <AchievementListModal
          opened={achievementsOpen}
          onClose={() => setAchievementsOpen(false)}
          entryKey={detail.row.key}
          gameTitle={detail.row.title}
        />
      )}
    </div>
  );
};
