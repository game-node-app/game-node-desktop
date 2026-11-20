import React from 'react';
import {
  IconDeviceDesktop,
  IconDownload,
  IconEyeOff,
  IconPlayerPlayFilled,
  IconStarFilled,
  IconUsersGroup,
  type TablerIcon,
} from '@tabler/icons-react';
import { coverAssetUrl, type CoverSize, type Row } from '../../api';
import { useI18n } from '../../i18n';
import { cx } from './cx';
import { GameCover } from './GameCover';
import {
  MATCH_DOT_CLASSES,
  formatCopy,
  formatShortPlaytime,
  matchLabel,
  matchTone,
} from './gamePresentation';

/** The main action the hover control offers; omit it when neither is available. */
export type GameTilePrimaryAction = 'play' | 'install';

export interface GameTileProps {
  /** The library row to render. Pass only rows the view wants visible. */
  row: Row;
  /** Selected by the engine's selection set. Draws the brand ring and tinted surface. */
  selected?: boolean;
  /** Keyboard focus computed by the engine; draws the 2 px focus ring. */
  focused?: boolean;
  /**
   * Remote cover URL for catalog results. When omitted, the cached local cover
   * of `row.coverFile` is used; an explicit `null` means no cover.
   */
  coverUrl?: string | null;
  /**
   * Primary hover action. Play is offered for launchable games, Install for
   * owned-but-not-installed ones; omit it while the game is running or when the
   * provider has no launch/install action.
   */
  primaryAction?: GameTilePrimaryAction;
  onPrimaryAction?: () => void;
  /**
   * Cover-size preference for the library grid. The cover keeps the 160×214
   * aspect and the caption height stays fixed; the list view ignores it.
   */
  size?: CoverSize;
  /** Store glyph supplied by the view; defaults to no glyph. */
  providerIcon?: TablerIcon;
  /**
   * Live session time shown in the running badge, e.g. "0:42:10". Compute it
   * from `row.sessionStartedAt` with `sessionElapsedSeconds` and
   * `formatSessionTime`; never time from the tile's mount.
   */
  runningTime?: string | null;
  /** Root pointer selection. The caller applies replace/toggle/range semantics. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onContextMenu?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onDoubleClick?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
  /**
   * Replaces the default library metadata row (match dot, store, playtime).
   * Catalog results pass their release year here; omitting the prop keeps the
   * library row, so the shared primitive still has no catalog knowledge.
   */
  meta?: React.ReactNode;
  /**
   * Replaces the accessible name built from library facts. Non-library callers
   * use it so the tile never announces a match or playtime it does not have.
   */
  accessibleName?: string;
  /**
   * Roving tab index. Defaults to 0 for the focused tile and -1 otherwise, so
   * the grid keeps one tab stop while any tile remains programmatically focusable.
   */
  tabIndex?: number;
  className?: string;
  /** Runtime layout values only, such as the virtualizer translation. */
  style?: React.CSSProperties;
  'data-testid'?: string;
}

const BADGE_BASE =
  'inline-flex h-[20px] max-w-full items-center gap-[4px] rounded-sm px-[6px] text-[11px] leading-none';

/**
 * Tile and cover boxes per `Cover size`: Small 140, Medium 172, Large 216 wide.
 * Heights are the 6 px padding + rounded 160×214-ratio cover + 8 px gap + the
 * 18 px title and 18 px meta lines, so only the cover area grows.
 */
const TILE_SIZE_CLASSES: Record<CoverSize, { tile: string; cover: string }> = {
  small: { tile: 'h-[227px] w-[140px]', cover: 'h-[171px] w-[128px]' },
  medium: { tile: 'h-[270px] w-[172px]', cover: 'h-[214px] w-[160px]' },
  large: { tile: 'h-[329px] w-[216px]', cover: 'h-[273px] w-[204px]' },
};

function TileBadge({
  tone,
  icon: Icon,
  iconClassName,
  children,
}: {
  tone: 'muted' | 'running';
  icon: TablerIcon;
  iconClassName?: string;
  children: React.ReactNode;
}) {
  return (
    <span
      className={cx(
        BADGE_BASE,
        tone === 'running'
          ? 'bg-success font-semibold text-canvas'
          : 'bg-black/60 font-medium text-white',
      )}
    >
      <Icon
        size={12}
        stroke={2}
        aria-hidden="true"
        className={cx('shrink-0', iconClassName)}
      />
      <span className="truncate">{children}</span>
    </span>
  );
}

/**
 * Grid tile from `components.md`: 172×270 at the default Medium cover size.
 * It is presentational: the row, selection/focus flags and callbacks arrive as
 * props, never from the engine or a command module. Covers are lazy and
 * decoded off the UI thread.
 */
export function GameTile({
  row,
  selected = false,
  focused = false,
  coverUrl,
  primaryAction,
  onPrimaryAction,
  size = 'medium',
  providerIcon: ProviderIcon,
  runningTime,
  onSelect,
  onContextMenu,
  onDoubleClick,
  onKeyDown,
  meta,
  accessibleName,
  tabIndex,
  className,
  style,
  'data-testid': testId = `game-tile-${row.key}`,
}: GameTileProps) {
  const { t } = useI18n();
  const src = coverUrl !== undefined ? coverUrl : coverAssetUrl(row.coverFile);
  const runningLabel = runningTime
    ? formatCopy(t('Library::Running · {}'), runningTime)
    : t('Library::Running');
  const match = matchLabel(row.status, t);
  const showPrimary = primaryAction != null && onPrimaryAction != null;
  const PrimaryIcon =
    primaryAction === 'install' ? IconDownload : IconPlayerPlayFilled;
  const primaryLabel =
    primaryAction === 'install' ? t('Library::Install') : t('Library::Play');
  const sizeClasses = TILE_SIZE_CLASSES[size];

  const accessibleParts = [row.title];
  if (row.installed) accessibleParts.push(t('Library::Installed'));
  if (row.running) accessibleParts.push(runningLabel);
  if (row.shared && !row.owned) accessibleParts.push(t('Library::Family'));
  if (row.isFavorite) accessibleParts.push(t('Library::Favorite'));
  if (row.hidden) accessibleParts.push(t('Library::Hidden'));
  accessibleParts.push(match);
  const ariaLabel =
    accessibleName !== undefined ? accessibleName : accessibleParts.join(' · ');

  const ringClass = focused
    ? 'ring-2 ring-inset ring-brand'
    : selected
      ? 'ring-[1.5px] ring-inset ring-brand'
      : '';

  return (
    <div
      data-testid={testId}
      data-selected={selected ? 'true' : undefined}
      data-focused={focused ? 'true' : undefined}
      data-hidden={row.hidden ? 'true' : undefined}
      role="listitem"
      aria-label={ariaLabel}
      aria-selected={selected}
      tabIndex={tabIndex ?? (focused ? 0 : -1)}
      onClick={onSelect}
      onContextMenu={onContextMenu}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKeyDown}
      style={style}
      className={cx(
        'group relative box-border flex shrink-0 select-none flex-col gap-[8px] rounded-md p-[6px] transition-colors',
        sizeClasses.tile,
        selected ? 'bg-accent-subtle' : 'bg-transparent',
        !selected && 'hover:bg-raised',
        ringClass,
        row.hidden && 'opacity-50',
        onSelect ? 'cursor-pointer' : 'cursor-default',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand',
        className,
      )}
    >
      <div
        className={cx(
          'relative shrink-0 overflow-hidden rounded-control',
          sizeClasses.cover,
        )}
      >
        <GameCover
          src={src}
          title={row.title}
          overlayTitle
          data-testid="game-tile-cover"
        />

        <div className="pointer-events-none absolute left-[6px] top-[6px] flex max-w-[148px] flex-col items-start gap-[4px]">
          {row.running ? (
            <TileBadge tone="running" icon={IconPlayerPlayFilled}>
              {runningLabel}
            </TileBadge>
          ) : row.installed ? (
            <TileBadge
              tone="muted"
              icon={IconDeviceDesktop}
              iconClassName="text-success"
            >
              {t('Library::Installed')}
            </TileBadge>
          ) : null}
          {row.shared && !row.owned ? (
            <TileBadge tone="muted" icon={IconUsersGroup}>
              {t('Library::Family')}
            </TileBadge>
          ) : null}
          {row.hidden ? (
            <TileBadge tone="muted" icon={IconEyeOff}>
              {t('Library::Hidden')}
            </TileBadge>
          ) : null}
        </div>

        {row.isFavorite ? (
          <span
            role="img"
            aria-label={t('Library::Favorite')}
            className="pointer-events-none absolute right-[6px] top-[6px] flex h-[22px] w-[22px] items-center justify-center rounded-full bg-black/60"
          >
            <IconStarFilled size={12} aria-hidden="true" className="text-warning" />
          </span>
        ) : null}

        {showPrimary ? (
          <>
            <div
              aria-hidden="true"
              className="pointer-events-none absolute inset-0 bg-black/40 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
            />
            <button
              type="button"
              tabIndex={-1}
              aria-label={primaryLabel}
              onClick={(event) => {
                event.stopPropagation();
                onPrimaryAction?.();
              }}
              onDoubleClick={(event) => event.stopPropagation()}
              className="absolute left-1/2 top-1/2 flex h-[48px] w-[48px] -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-brand text-white opacity-0 transition-opacity hover:bg-[var(--color-primary-hover)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-white group-hover:opacity-100 group-focus-within:opacity-100"
            >
              <PrimaryIcon size={20} aria-hidden="true" />
            </button>
          </>
        ) : null}
      </div>

      <div className="flex min-w-0 flex-col">
        <span
          title={row.title}
          className={cx(
            'truncate text-sm font-medium leading-[18px]',
            selected ? 'text-text-primary' : 'text-text-default',
          )}
        >
          {row.title}
        </span>
        {meta !== undefined ? (
          <div className="flex h-[18px] min-w-0 items-center gap-[6px] text-xs text-text-dimmed">
            {meta}
          </div>
        ) : (
          <div className="flex h-[18px] min-w-0 items-center gap-[6px] text-xs text-text-dimmed">
            <span
              aria-hidden="true"
              className={cx(
                'h-[6px] w-[6px] shrink-0 rounded-full',
                MATCH_DOT_CLASSES[matchTone(row.status)],
              )}
            />
            <span className="sr-only">{match}</span>
            {ProviderIcon ? (
              <ProviderIcon
                size={12}
                stroke={2}
                aria-hidden="true"
                className="shrink-0"
              />
            ) : null}
            <span className="truncate">{row.providerName}</span>
            <span className="ml-auto shrink-0 tabular-nums">
              {formatShortPlaytime(row.playtime?.seconds, t)}
            </span>
          </div>
        )}
      </div>
    </div>
  );
}
