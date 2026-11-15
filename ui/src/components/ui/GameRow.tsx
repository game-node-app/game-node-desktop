import React, { useId } from 'react';
import {
  IconDeviceDesktop,
  IconDownload,
  IconEyeOff,
  IconPlayerPlayFilled,
  IconStar,
  IconStarFilled,
  IconUsersGroup,
  type TablerIcon,
} from '@tabler/icons-react';
import { coverAssetUrl, type Row } from '../../api';
import { useI18n } from '../../i18n';
import { Checkbox } from './Checkbox';
import { GameCover } from './GameCover';
import { Pill } from './Pill';
import { cx } from './cx';
import {
  MATCH_DOT_CLASSES,
  formatCopy,
  formatLastSession,
  formatLongPlaytime,
  gameNodeStatusView,
  isMatchedStatus,
  matchLabel,
  matchTone,
} from './gamePresentation';

/**
 * Shared list geometry from `components.md` and
 * `raw/04-L2-list-grouped-multiselect.txt`. The title column fills the
 * remaining width; every other column is fixed. `GameList` reuses this object
 * for its sticky 34 px header. `paddingX` is the content inset including the
 * 1 px selection border, which is why the row uses `px-[11px]` and the header
 * (with no side border) uses `px-[12px]` for identical column x positions.
 */
export const GAME_ROW_LAYOUT = {
  height: 52,
  /** Content inset from the row border, including its 1 px selection border. */
  paddingX: 12,
  gap: 12,
  columns: {
    checkbox: 16,
    cover: 30,
    store: 130,
    gameNodeStatus: 120,
    match: 130,
    playtime: 90,
    lastSession: 110,
    favorite: 28,
  },
} as const;

/** Title floor before the shared list surface scrolls horizontally. */
export const GAME_ROW_TITLE_MIN_WIDTH = 200;

const GAME_ROW_FIXED_WIDTH = Object.values(GAME_ROW_LAYOUT.columns).reduce(
  (total, width) => total + width,
  0,
);

/**
 * Width the header and the rows need for every fixed column and the 200 px
 * title floor to fit. `paddingX` already includes the 1 px row border, so the
 * row's inner padding is 11 px and its content starts on the same 12 px inset
 * as the header. Below this width the list scrolls horizontally instead of
 * clipping the trailing columns.
 */
export const GAME_ROW_MIN_WIDTH =
  GAME_ROW_LAYOUT.paddingX * 2 +
  GAME_ROW_LAYOUT.gap * Object.keys(GAME_ROW_LAYOUT.columns).length +
  GAME_ROW_FIXED_WIDTH +
  GAME_ROW_TITLE_MIN_WIDTH;

export interface GameRowProps {
  /** The library row to render. */
  row: Row;
  selected?: boolean;
  /** Keyboard focus computed by the engine; draws the 2 px focus ring. */
  focused?: boolean;
  /**
   * Whether the account library has ever been read. While it has not, the
   * GameNode status cell stays empty, per `desktop-account-library`.
   */
  gameNodeStatusAvailable?: boolean;
  /** Store glyph supplied by the view; defaults to no glyph. */
  providerIcon?: TablerIcon;
  /** Live session time shown in the running flag, e.g. "0:42:10". */
  runningTime?: string | null;
  /** Unix seconds used by the relative last-session label. */
  nowSeconds?: number;
  /** Root pointer selection. The caller applies replace/toggle/range semantics. */
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onContextMenu?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onDoubleClick?: (event: React.MouseEvent<HTMLDivElement>) => void;
  onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
  /** Checkbox toggle. Clicking the box never selects the row via `onSelect`. */
  onToggleSelected?: (selected: boolean) => void;
  /** Favorite toggle. Omit it to disable the column button. */
  onToggleFavorite?: () => void;
  /** Roving tab index. Defaults to 0 for the focused row and -1 otherwise. */
  tabIndex?: number;
  className?: string;
  /** Runtime layout values only, such as the virtualizer translation. */
  style?: React.CSSProperties;
  'data-testid'?: string;
}

function Flag({
  icon: Icon,
  label,
  iconClassName,
}: {
  icon: TablerIcon;
  label: string;
  iconClassName: string;
}) {
  return (
    <span className="inline-flex shrink-0 items-center gap-[4px]">
      <Icon
        size={12}
        stroke={2}
        aria-hidden="true"
        className={cx('shrink-0', iconClassName)}
      />
      <span className="truncate">{label}</span>
    </span>
  );
}

/**
 * 52 px library list row from `components.md`. It is presentational: the row,
 * selection/focus flags and pointer callbacks arrive as props, never from the
 * engine or a command module. Covers are lazy and decoded off the UI thread.
 */
export function GameRow({
  row,
  selected = false,
  focused = false,
  gameNodeStatusAvailable = true,
  providerIcon: ProviderIcon,
  runningTime,
  nowSeconds = Math.floor(Date.now() / 1000),
  onSelect,
  onContextMenu,
  onDoubleClick,
  onKeyDown,
  onToggleSelected,
  onToggleFavorite,
  tabIndex,
  className,
  style,
  'data-testid': testId = `game-row-${row.key}`,
}: GameRowProps) {
  const { t, locale } = useI18n();
  const titleId = useId();
  const matched = isMatchedStatus(row.status);
  const statusView = row.gamenode ? gameNodeStatusView(row.gamenode.status, t) : null;
  const runningLabel = runningTime
    ? formatCopy(t('Library::Running · {}'), runningTime)
    : t('Library::Running');
  const favoriteLabel = row.isFavorite
    ? t('Library::Remove from favorites')
    : t('Library::Add to favorites');

  const flags: React.ReactNode[] = [];
  if (row.running) {
    flags.push(
      <Flag
        key="running"
        icon={IconPlayerPlayFilled}
        label={runningLabel}
        iconClassName="text-success"
      />,
    );
  } else if (row.installed) {
    flags.push(
      <Flag
        key="installed"
        icon={IconDeviceDesktop}
        label={t('Library::Installed')}
        iconClassName="text-success"
      />,
    );
  } else {
    flags.push(
      <Flag
        key="not-installed"
        icon={IconDownload}
        label={t('Library::Not installed')}
        iconClassName="text-text-secondary"
      />,
    );
  }
  if (row.shared && !row.owned) {
    flags.push(
      <Flag
        key="shared"
        icon={IconUsersGroup}
        label={t('Library::Family shared')}
        iconClassName="text-text-secondary"
      />,
    );
  }
  if (row.hidden) {
    flags.push(
      <Flag
        key="hidden"
        icon={IconEyeOff}
        label={t('Library::Hidden')}
        iconClassName="text-text-secondary"
      />,
    );
  }

  const favoriteDisabled = !matched || onToggleFavorite == null;

  return (
    <div
      data-testid={testId}
      data-selected={selected ? 'true' : undefined}
      data-focused={focused ? 'true' : undefined}
      data-hidden={row.hidden ? 'true' : undefined}
      role="listitem"
      aria-selected={selected}
      tabIndex={tabIndex ?? (focused ? 0 : -1)}
      onClick={onSelect}
      onContextMenu={onContextMenu}
      onDoubleClick={onDoubleClick}
      onKeyDown={onKeyDown}
      style={style}
      className={cx(
        'flex h-[52px] w-full items-center gap-[12px] rounded-control border px-[11px] transition-colors',
        selected
          ? 'border-brand bg-accent-subtle'
          : 'border-transparent bg-transparent hover:bg-raised',
        focused && 'ring-2 ring-inset ring-brand',
        row.hidden && 'opacity-50',
        onSelect ? 'cursor-pointer' : 'cursor-default',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand',
        className,
      )}
    >
      <div
        className="flex w-[16px] shrink-0 items-center justify-center"
        onClick={(event) => event.stopPropagation()}
        onDoubleClick={(event) => event.stopPropagation()}
      >
        <Checkbox
          checked={selected}
          aria-labelledby={titleId}
          onChange={(event) => onToggleSelected?.(event.currentTarget.checked)}
        />
      </div>

      <div className="h-[40px] w-[30px] shrink-0 overflow-hidden rounded-thumb">
        <GameCover
          src={coverAssetUrl(row.coverFile)}
          title={row.title}
          data-testid="game-row-cover"
        />
      </div>

      <div className="flex min-w-0 flex-1 flex-col justify-center">
        <span
          id={titleId}
          title={row.title}
          className="truncate text-sm font-medium text-text-default"
        >
          {row.title}
        </span>
        <div className="flex min-w-0 items-center gap-[6px] overflow-hidden whitespace-nowrap text-xs text-text-dimmed">
          {flags.map((flag, index) => (
            <React.Fragment key={index}>
              {index > 0 ? <span aria-hidden="true">·</span> : null}
              {flag}
            </React.Fragment>
          ))}
        </div>
      </div>

      <div className="flex w-[130px] shrink-0 items-center gap-[6px] text-body">
        {ProviderIcon ? (
          <ProviderIcon
            size={14}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-text-secondary"
          />
        ) : null}
        <span className="truncate text-text-default">{row.providerName}</span>
      </div>

      <div className="w-[120px] shrink-0">
        {gameNodeStatusAvailable ? (
          matched && statusView ? (
            <Pill tone={statusView.tone}>{statusView.label}</Pill>
          ) : (
            <span className="text-[13px] text-text-dimmed">—</span>
          )
        ) : null}
      </div>

      <div className="flex w-[130px] shrink-0 items-center gap-[6px] text-[13px]">
        <span
          aria-hidden="true"
          className={cx(
            'h-[6px] w-[6px] shrink-0 rounded-full',
            MATCH_DOT_CLASSES[matchTone(row.status)],
          )}
        />
        <span className="truncate text-text-default">{matchLabel(row.status, t)}</span>
      </div>

      <div className="w-[90px] shrink-0 text-[13px] text-text-default">
        {formatLongPlaytime(row.playtime?.seconds, t)}
      </div>

      <div className="w-[110px] shrink-0 text-[13px] text-text-default">
        {formatLastSession(row.playtime?.lastPlayedAt, t, nowSeconds, locale)}
      </div>

      <div className="flex w-[28px] shrink-0 items-center justify-end">
        <button
          type="button"
          aria-label={favoriteLabel}
          aria-pressed={row.isFavorite}
          disabled={favoriteDisabled}
          title={
            !matched
              ? t('Library::Match the game first to use favorites.')
              : undefined
          }
          onClick={(event) => {
            event.stopPropagation();
            onToggleFavorite?.();
          }}
          onDoubleClick={(event) => event.stopPropagation()}
          className="flex h-[28px] w-[28px] items-center justify-center rounded-control transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50"
        >
          {row.isFavorite ? (
            <IconStarFilled size={14} aria-hidden="true" className="text-warning" />
          ) : (
            <IconStar size={14} aria-hidden="true" className="text-text-muted" />
          )}
        </button>
      </div>
    </div>
  );
}
