import { IconChevronDown, IconChevronRight, type TablerIcon } from '@tabler/icons-react';
import { cx } from './cx';

export interface GroupHeaderProps {
  label: string;
  /** Raw count, used when `countLabel` is omitted. */
  count: number;
  /** Localized count text, for example "164 games". */
  countLabel?: string;
  /** Group icon shown after the chevron. */
  icon?: TablerIcon;
  collapsed: boolean;
  onToggle: () => void;
  className?: string;
}

/**
 * Collapsible group header shared by the grid and list. The button owns
 * `aria-expanded`; the chevron and rule are presentational.
 */
export function GroupHeader({
  label,
  count,
  countLabel,
  icon: Icon,
  collapsed,
  onToggle,
  className,
}: GroupHeaderProps) {
  const Chevron = collapsed ? IconChevronRight : IconChevronDown;

  return (
    <button
      type="button"
      aria-expanded={!collapsed}
      onClick={onToggle}
      className={cx(
        'flex h-[36px] w-full items-center gap-[8px] px-[4px] text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand focus-visible:ring-offset-1 focus-visible:ring-offset-body',
        className,
      )}
    >
      <Chevron size={14} stroke={2} aria-hidden="true" className="shrink-0 text-text-dimmed" />
      {Icon ? (
        <Icon size={16} stroke={2} aria-hidden="true" className="shrink-0 text-text-secondary" />
      ) : null}
      <span className="shrink-0 text-[13px] font-semibold text-text-primary">{label}</span>
      <span className="shrink-0 text-xs font-normal text-text-dimmed">
        {countLabel ?? count}
      </span>
      <span aria-hidden="true" className="h-px min-w-4 flex-1 bg-border-subtle" />
    </button>
  );
}
