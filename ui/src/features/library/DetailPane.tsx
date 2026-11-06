import React from 'react';
import {
  commands,
  useLibraryView,
  useSnapshot,
  useTools,
  artworkAssetUrl,
  getMatchedGameId,
  type Membership,
} from '../../api';
import { useUiStore, uiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { CoverImage } from './CoverImage';
import {
  formatI18n,
  formatReason,
  formatStatus,
  formatDuration,
  formatMatchStatus,
} from './formatters';

export const DetailPane: React.FC = () => {
  const library = useLibraryView();
  const tools = useTools();
  const { data: launch } = useSnapshot((s) => s.launch);
  const detailDragWidth = useUiStore((s) => s.detailDragWidth);
  const { t, locale } = useI18n();

  const layout = library?.layout;
  const detail = library?.detail;
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

  return (
    <div
      data-testid="detail-pane"
      style={{
        display: 'flex',
        height: '100%',
        position: 'relative',
        flexShrink: 0,
      }}
    >
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
        style={{
          width: '8px',
          height: '100%',
          cursor: 'col-resize',
          backgroundColor: 'transparent',
          position: 'relative',
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'center',
          userSelect: 'none',
        }}
      >
        <div
          style={{
            width: '1px',
            height: '100%',
            backgroundColor: 'var(--color-border-subtle, #2A2A2A)',
          }}
        />
      </div>

      {/* Pane Content */}
      <div
        style={{
          width: `${clampedWidth}px`,
          height: '100%',
          backgroundColor: 'var(--color-secondary, #1A1A1A)',
          borderLeft: '1px solid var(--color-border-subtle, #2A2A2A)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        {/* Header */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            padding: '12px 16px',
            borderBottom: '1px solid var(--color-border-subtle, #2A2A2A)',
            flexShrink: 0,
          }}
        >
          <span
            style={{
              fontSize: 'var(--font-size-lg, 18px)',
              fontWeight: 600,
              color: 'var(--color-text-default, #D9D9D9)',
            }}
          >
            {t('Library::Details')}
          </span>
          <button
            type="button"
            data-testid="detail-close-btn"
            aria-label="Close"
            onClick={handleClose}
            style={{
              background: 'transparent',
              border: 'none',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
              fontSize: '20px',
              lineHeight: 1,
              padding: '4px 8px',
              borderRadius: 'var(--radius-sm, 4px)',
            }}
          >
            ×
          </button>
        </div>

        {/* Scrollable Body */}
        <div
          style={{
            flex: 1,
            overflowY: 'auto',
            padding: '16px',
            display: 'flex',
            flexDirection: 'column',
            gap: '16px',
          }}
        >
          {/* Multiple selected state */}
          {!detail && selectedCount > 1 && (
            <div data-testid="detail-multiple-selected">
              <h3
                style={{
                  fontSize: 'var(--font-size-md, 16px)',
                  color: 'var(--color-text-default, #D9D9D9)',
                  marginBottom: '12px',
                }}
              >
                {formatI18n(t('Library::{} games selected'), selectedCount)}
              </h3>
              <div
                style={{
                  backgroundColor: 'var(--color-raised, #222222)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '12px',
                }}
              >
                <button
                  type="button"
                  data-testid="detail-add-multiple-btn"
                  disabled={actions?.addToGamenode !== 'available'}
                  onClick={() => commands.libraryInput('openAdd')}
                  style={{
                    width: '100%',
                    padding: '8px 12px',
                    backgroundColor: 'var(--color-primary, #F15025)',
                    color: 'var(--color-on-primary, #EDEDED)',
                    border: 'none',
                    borderRadius: 'var(--radius-sm, 4px)',
                    cursor: actions?.addToGamenode === 'available' ? 'pointer' : 'default',
                    opacity: actions?.addToGamenode === 'available' ? 1 : 0.5,
                  }}
                >
                  {t('Library::Add to GameNode…')}
                </button>
                {actions?.addToGamenode !== 'available' && (
                  <div
                    data-testid="detail-add-multiple-reason"
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      color: 'var(--color-warning, #F5A524)',
                      marginTop: '6px',
                    }}
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
              style={{
                color: 'var(--color-text-dimmed, #808080)',
                fontSize: 'var(--font-size-sm, 14px)',
                textAlign: 'center',
                paddingTop: '32px',
              }}
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
                style={{
                  width: '100%',
                  height: '180px',
                  borderRadius: 'var(--radius-sm, 4px)',
                  overflow: 'hidden',
                  backgroundColor: 'var(--color-surface, #262525)',
                  flexShrink: 0,
                }}
              >
                {detail.hasArtwork && getMatchedGameId(detail.row.status) != null ? (
                  <img
                    data-testid="detail-artwork-img"
                    src={artworkAssetUrl(getMatchedGameId(detail.row.status))!}
                    alt={detail.row.title}
                    loading="lazy"
                    decoding="async"
                    style={{
                      width: '100%',
                      height: '100%',
                      objectFit: 'cover',
                      display: 'block',
                    }}
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
                    style={{
                      color: 'var(--color-success, #3DD68C)',
                      fontSize: 'var(--font-size-xs, 12px)',
                      fontWeight: 600,
                      marginBottom: '4px',
                    }}
                  >
                    {t('Library::Running')}
                  </div>
                )}
                <h2
                  data-testid="detail-title"
                  style={{
                    fontSize: 'var(--font-size-lg, 18px)',
                    fontWeight: 600,
                    color: 'var(--color-text-default, #D9D9D9)',
                    lineHeight: 1.3,
                  }}
                >
                  {detail.row.title}
                </h2>
                <div
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    color: 'var(--color-text-dimmed, #808080)',
                    marginTop: '4px',
                  }}
                >
                  {detail.releaseYear ? `${detail.releaseYear} · ` : ''}
                  {detail.row.providerName}
                </div>
              </div>

              {/* Launch Status Banner */}
              {launch && (
                <div
                  data-testid="detail-launch-status"
                  style={{
                    backgroundColor:
                      launch.state === 'opening'
                        ? 'var(--color-accent-subtle, #F150251F)'
                        : 'var(--color-danger-subtle, #E5484D1F)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    padding: '10px 12px',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: '8px',
                  }}
                >
                  <span
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      color: 'var(--color-text-default, #D9D9D9)',
                    }}
                  >
                    {getLaunchMessage()}
                  </span>
                  {launch.state !== 'opening' && (
                    <button
                      type="button"
                      data-testid="detail-dismiss-launch-btn"
                      onClick={() => commands.dismissLaunch()}
                      style={{
                        alignSelf: 'flex-start',
                        padding: '4px 8px',
                        fontSize: 'var(--font-size-xs, 12px)',
                        backgroundColor: 'transparent',
                        border: '1px solid var(--color-border-subtle, #2A2A2A)',
                        color: 'var(--color-text-default, #D9D9D9)',
                        borderRadius: 'var(--radius-xs, 2px)',
                        cursor: 'pointer',
                      }}
                    >
                      {t('Library::Dismiss')}
                    </button>
                  )}
                </div>
              )}

              {/* Primary Actions (Play, Install) */}
              <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                {/* Play Button */}
                <div>
                  <button
                    type="button"
                    data-testid="detail-play-btn"
                    disabled={actions?.play !== 'available'}
                    onClick={() => commands.launchSelected('play')}
                    style={{
                      width: '100%',
                      padding: '10px 16px',
                      backgroundColor: 'var(--color-primary, #F15025)',
                      color: 'var(--color-on-primary, #EDEDED)',
                      border: 'none',
                      borderRadius: 'var(--radius-sm, 4px)',
                      fontSize: 'var(--font-size-sm, 14px)',
                      fontWeight: 600,
                      cursor: actions?.play === 'available' ? 'pointer' : 'default',
                      opacity: actions?.play === 'available' ? 1 : 0.45,
                    }}
                  >
                    {t('Library::Play')}
                  </button>
                  {actions?.play !== 'available' && (
                    <div
                      data-testid="detail-play-reason"
                      style={{
                        fontSize: 'var(--font-size-xs, 12px)',
                        color: 'var(--color-warning, #F5A524)',
                        marginTop: '4px',
                      }}
                    >
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
                        disabled={actions.install !== 'available'}
                        onClick={() => commands.launchSelected('install')}
                        style={{
                          width: '100%',
                          padding: '10px 16px',
                          backgroundColor: 'transparent',
                          color: 'var(--color-text-default, #D9D9D9)',
                          border: '1px solid var(--color-border-subtle, #2A2A2A)',
                          borderRadius: 'var(--radius-sm, 4px)',
                          fontSize: 'var(--font-size-sm, 14px)',
                          fontWeight: 500,
                          cursor: actions.install === 'available' ? 'pointer' : 'default',
                          opacity: actions.install === 'available' ? 1 : 0.45,
                        }}
                      >
                        {t('Library::Install')}
                      </button>
                      {actions.install !== 'available' && (
                        <div
                          data-testid="detail-install-reason"
                          style={{
                            fontSize: 'var(--font-size-xs, 12px)',
                            color: 'var(--color-warning, #F5A524)',
                            marginTop: '4px',
                          }}
                        >
                          {formatReason(actions.install.disabled, t)}
                        </div>
                      )}
                    </div>
                  )}
              </div>

              {/* Playtime Row */}
              <div
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  padding: '8px 12px',
                  backgroundColor: 'var(--color-raised, #222222)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  fontSize: 'var(--font-size-xs, 12px)',
                }}
              >
                <span style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                  {t('Library::Playtime')}
                </span>
                <span
                  data-testid="detail-playtime-val"
                  style={{
                    color: 'var(--color-text-default, #D9D9D9)',
                    fontWeight: 500,
                  }}
                >
                  {formatDuration(detail.row.playtime?.seconds, t)}
                </span>
                <span style={{ color: 'var(--color-text-dimmed, #808080)', fontSize: '11px' }}>
                  {detail.row.playtime?.source === 'store'
                    ? formatI18n(t('Library::Imported from {}'), detail.row.providerName)
                    : t('Library::Internal counter')}
                </span>
              </div>

              {/* GameNode Card */}
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '6px',
                }}
              >
                <span
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    fontWeight: 600,
                    color: 'var(--color-text-dimmed, #808080)',
                    textTransform: 'uppercase',
                    letterSpacing: '0.5px',
                  }}
                >
                  {t('Library::GameNode')}
                </span>

                <div
                  style={{
                    backgroundColor: 'var(--color-raised, #222222)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    padding: '8px 12px',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: '8px',
                  }}
                >
                  {/* Match / Change Match */}
                  <div>
                    <button
                      type="button"
                      data-testid="detail-match-btn"
                      disabled={actions?.matchGame !== 'available'}
                      onClick={() => commands.libraryInput('openMatch')}
                      style={{
                        width: '100%',
                        padding: '8px 10px',
                        backgroundColor: 'transparent',
                        border: '1px solid var(--color-border-subtle, #2A2A2A)',
                        borderRadius: 'var(--radius-xs, 2px)',
                        color: 'var(--color-text-default, #D9D9D9)',
                        textAlign: 'left',
                        cursor: actions?.matchGame === 'available' ? 'pointer' : 'default',
                        opacity: actions?.matchGame === 'available' ? 1 : 0.45,
                        fontSize: 'var(--font-size-sm, 14px)',
                      }}
                    >
                      {actions?.changeMatch
                        ? t('Library::Change match…')
                        : t('Library::Match…')}
                    </button>
                    {actions?.matchGame !== 'available' && (
                      <div
                        data-testid="detail-match-reason"
                        style={{
                          fontSize: 'var(--font-size-xs, 12px)',
                          color: 'var(--color-warning, #F5A524)',
                          marginTop: '4px',
                        }}
                      >
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
                      style={{
                        width: '100%',
                        padding: '8px 10px',
                        backgroundColor: 'transparent',
                        border: '1px solid var(--color-border-subtle, #2A2A2A)',
                        borderRadius: 'var(--radius-xs, 2px)',
                        color: 'var(--color-text-default, #D9D9D9)',
                        textAlign: 'left',
                        cursor: actions?.addToGamenode === 'available' ? 'pointer' : 'default',
                        opacity: actions?.addToGamenode === 'available' ? 1 : 0.45,
                        fontSize: 'var(--font-size-sm, 14px)',
                      }}
                    >
                      {typeof detail.membership === 'object' &&
                      'inLibrary' in detail.membership
                        ? t('Library::Edit in GameNode…')
                        : t('Library::Add to GameNode…')}
                    </button>
                    {actions?.addToGamenode !== 'available' && (
                      <div
                        data-testid="detail-add-reason"
                        style={{
                          fontSize: 'var(--font-size-xs, 12px)',
                          color: 'var(--color-warning, #F5A524)',
                          marginTop: '4px',
                        }}
                      >
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
                        style={{
                          alignSelf: 'flex-start',
                          padding: '4px 8px',
                          fontSize: 'var(--font-size-xs, 12px)',
                          backgroundColor: 'transparent',
                          border: '1px solid var(--color-border-subtle, #2A2A2A)',
                          color: 'var(--color-text-default, #D9D9D9)',
                          borderRadius: 'var(--radius-xs, 2px)',
                          cursor: 'pointer',
                        }}
                      >
                        {t('Library::Check again')}
                      </button>
                    )}
                </div>
              </div>

              {/* Manage Card */}
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '6px',
                }}
              >
                <span
                  style={{
                    fontSize: 'var(--font-size-xs, 12px)',
                    fontWeight: 600,
                    color: 'var(--color-text-dimmed, #808080)',
                    textTransform: 'uppercase',
                    letterSpacing: '0.5px',
                  }}
                >
                  {t('Library::Manage')}
                </span>

                <div
                  style={{
                    backgroundColor: 'var(--color-raised, #222222)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    padding: '8px 12px',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: '8px',
                  }}
                >
                  {/* Game Tools */}
                  <div>
                    <button
                      type="button"
                      data-testid="detail-tools-btn"
                      disabled={detail.running || detail.row.running}
                      onClick={() => commands.openTools(detail.row.key)}
                      style={{
                        width: '100%',
                        padding: '8px 10px',
                        backgroundColor: 'transparent',
                        border: '1px solid var(--color-border-subtle, #2A2A2A)',
                        borderRadius: 'var(--radius-xs, 2px)',
                        color: 'var(--color-text-default, #D9D9D9)',
                        textAlign: 'left',
                        cursor: !(detail.running || detail.row.running)
                          ? 'pointer'
                          : 'default',
                        opacity: !(detail.running || detail.row.running) ? 1 : 0.45,
                        fontSize: 'var(--font-size-sm, 14px)',
                      }}
                    >
                      {t('Library::Game tools…')}
                    </button>
                    {/* Tools installed summary */}
                    {tools?.installedByEntry?.[detail.row.key]?.length ? (
                      <div
                        data-testid="detail-tools-summary"
                        style={{
                          fontSize: 'var(--font-size-xs, 12px)',
                          color: 'var(--color-text-dimmed, #808080)',
                          marginTop: '4px',
                        }}
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
                        style={{
                          width: '100%',
                          padding: '8px 10px',
                          backgroundColor: 'transparent',
                          border: '1px solid var(--color-border-subtle, #2A2A2A)',
                          borderRadius: 'var(--radius-xs, 2px)',
                          color: 'var(--color-text-default, #D9D9D9)',
                          textAlign: 'left',
                          cursor: 'pointer',
                          fontSize: 'var(--font-size-sm, 14px)',
                        }}
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
                        style={{
                          width: '100%',
                          padding: '8px 10px',
                          backgroundColor: 'transparent',
                          border: '1px solid var(--color-border-subtle, #2A2A2A)',
                          borderRadius: 'var(--radius-xs, 2px)',
                          color: 'var(--color-text-default, #D9D9D9)',
                          textAlign: 'left',
                          cursor: 'pointer',
                          fontSize: 'var(--font-size-sm, 14px)',
                        }}
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
                        style={{
                          width: '100%',
                          padding: '8px 10px',
                          backgroundColor: 'transparent',
                          border: '1px solid var(--color-border-subtle, #2A2A2A)',
                          borderRadius: 'var(--radius-xs, 2px)',
                          color: 'var(--color-text-default, #D9D9D9)',
                          textAlign: 'left',
                          cursor: 'pointer',
                          fontSize: 'var(--font-size-sm, 14px)',
                        }}
                      >
                        {t('DRM-free::Locate executable')}
                      </button>
                    </>
                  )}
                </div>
              </div>

              {/* Information Card */}
              <div
                style={{
                  backgroundColor: 'var(--color-raised, #222222)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '12px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '10px',
                  fontSize: 'var(--font-size-xs, 12px)',
                }}
              >
                {/* Detected title */}
                <div>
                  <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                    {t('Library::Detected title')}
                  </div>
                  <div
                    data-testid="detail-detected-title"
                    style={{
                      color: 'var(--color-text-default, #D9D9D9)',
                      fontWeight: 500,
                    }}
                  >
                    {detail.row.detectedName}
                  </div>
                </div>

                {/* GameNode game */}
                <div>
                  <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                    {t('Library::GameNode game')}
                  </div>
                  <div
                    data-testid="detail-gamenode-title"
                    style={{
                      color: 'var(--color-text-default, #D9D9D9)',
                      fontWeight: 500,
                    }}
                  >
                    {detail.row.matchedName || '—'}
                  </div>
                </div>

                {/* Install folder */}
                <div>
                  <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                    {t('Library::Install folder')}
                  </div>
                  <div
                    data-testid="detail-install-dir"
                    style={{
                      color: 'var(--color-text-default, #D9D9D9)',
                      wordBreak: 'break-all',
                    }}
                  >
                    {detail.installDir}
                  </div>
                </div>

                {/* Match */}
                <div>
                  <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                    {t('Library::Match')}
                  </div>
                  <div
                    data-testid="detail-match-status"
                    style={{ color: 'var(--color-text-default, #D9D9D9)' }}
                  >
                    {formatMatchStatus(detail.row.status, t)}
                  </div>
                </div>

                {/* In your GameNode library */}
                <div>
                  <div style={{ color: 'var(--color-text-dimmed, #808080)' }}>
                    {t('Library::In your GameNode library')}
                  </div>
                  <div
                    data-testid="detail-membership-standing"
                    style={{ color: 'var(--color-text-default, #D9D9D9)' }}
                  >
                    {formatMembershipStanding(detail.membership)}
                  </div>
                </div>
              </div>

              {/* Remove Action (for manual games) */}
              {detail.row.key.startsWith('manual:') && (
                <div
                  style={{
                    backgroundColor: 'var(--color-raised, #222222)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    padding: '8px 12px',
                  }}
                >
                  <button
                    type="button"
                    data-testid="detail-remove-btn"
                    onClick={() =>
                      uiStore.setState((prev) => ({
                        ...prev,
                        removeModalEntryKey: detail.row.key,
                      }))
                    }
                    style={{
                      width: '100%',
                      padding: '8px 10px',
                      backgroundColor: 'transparent',
                      border: '1px solid var(--color-danger, #E5484D)',
                      borderRadius: 'var(--radius-xs, 2px)',
                      color: 'var(--color-danger, #E5484D)',
                      textAlign: 'left',
                      cursor: 'pointer',
                      fontSize: 'var(--font-size-sm, 14px)',
                    }}
                  >
                    {locale === 'pt-BR'
                      ? t('DRM-free::Remove from library')
                      : 'Remove'}
                  </button>
                </div>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
};
