import React from 'react';
import { cx } from '../../../components/ui/cx';

export interface SettingsPanelProps
  extends Omit<React.HTMLAttributes<HTMLElement>, 'title'> {
  /** 22/600 panel title; it also names the region. */
  title: React.ReactNode;
  /** 13/400 panel description under the title. */
  description?: React.ReactNode;
  /** Trailing header action (`Button sm` or a `SegmentedControl`). */
  action?: React.ReactNode;
  /** Overrides the generated heading id used by `aria-labelledby`. */
  headingId?: string;
}

/**
 * Panel header plus content for one Settings topic (`components.md` §3).
 * The whole block is a labelled region so the modal shell can point its
 * scroller at it; the title and description scroll with the content.
 */
export function SettingsPanel({
  title,
  description,
  action,
  headingId,
  className,
  children,
  'aria-labelledby': ariaLabelledBy,
  ...props
}: SettingsPanelProps) {
  const generatedId = React.useId();
  const titleId = headingId ?? `${generatedId}-title`;

  const heading = (
    <>
      <h2
        id={titleId}
        className="text-heading font-semibold text-text-primary"
      >
        {title}
      </h2>
      {description ? (
        <p className="text-body text-text-secondary">{description}</p>
      ) : null}
    </>
  );

  return (
    <section
      {...props}
      aria-labelledby={ariaLabelledBy ?? titleId}
      className={cx('flex flex-col gap-lg', className)}
    >
      <header
        className={cx(
          action ? 'flex items-end gap-sm' : 'flex flex-col gap-[4px]',
        )}
      >
        {action ? (
          <div className="flex min-w-0 flex-1 flex-col gap-[4px]">
            {heading}
          </div>
        ) : (
          heading
        )}
        {action ? <div className="ml-auto shrink-0">{action}</div> : null}
      </header>
      {children}
    </section>
  );
}
