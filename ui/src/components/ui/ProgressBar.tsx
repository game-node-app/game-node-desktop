import { cx } from './cx';

export interface ProgressBarProps {
  /** Current value, from 0 to `max`. */
  value: number;
  max?: number;
  /** Localized accessible name, required so the bar is never unlabelled. */
  label: string;
  className?: string;
}

/**
 * 6px progress track from `components.md` §5. The fill width is the only
 * runtime value, so it stays an inline style; every color and radius comes
 * from tokens. The accessible values are clamped to the valid range so a
 * transient counter can never announce an impossible percentage.
 */
export function ProgressBar({
  value,
  max = 100,
  label,
  className,
}: ProgressBarProps) {
  const safeMax = max > 0 ? max : 0;
  const clamped = Math.min(Math.max(value, 0), safeMax);
  const percent = safeMax > 0 ? (clamped / safeMax) * 100 : 0;

  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={safeMax}
      aria-valuenow={clamped}
      className={cx(
        'h-[6px] w-full overflow-hidden rounded-thumb bg-raised',
        className,
      )}
    >
      <div
        className="h-full rounded-thumb bg-brand"
        style={{ width: `${percent}%` }}
      />
    </div>
  );
}
