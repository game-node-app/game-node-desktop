import React from 'react';
import type { TablerIcon } from '@tabler/icons-react';
import { cx } from '../../../components/ui/cx';
import { useI18n } from '../../../i18n';
import { useSettingsSearchMatch } from './SettingsSearchContext';

/** ids a row exposes so its control can be labelled and described. */
export interface SettingsRowControlIds {
  labelId: string;
  helpId: string;
  changedId: string;
  /** Space-separated help + "Changed" ids, ready for `aria-describedby`. */
  describedBy?: string;
}

export interface SettingsRowProps
  extends Omit<React.HTMLAttributes<HTMLDivElement>, 'children'> {
  /** 14/500 primary label; also labels the control through `controlIds`. */
  label: React.ReactNode;
  /** 12/400 dimmed help line, capped at the 400px settings measure. */
  help?: React.ReactNode;
  /** Optional leading icon; 18px, secondary (warning when unavailable). */
  icon?: TablerIcon;
  iconTone?: 'default' | 'warning';
  /** Control slot; a render prop receives the ids for `aria-*` wiring. */
  control?:
    | React.ReactNode
    | ((ids: SettingsRowControlIds) => React.ReactNode);
  /** Shows the brand changed dot and exposes "Changed" as a description. */
  changed?: boolean;
  /** Saved value shown as "Before: …" while `changed` is set. */
  previous?: string;
  /** Compact list row: 14px gap and 12/16 padding. */
  compact?: boolean;
  /** 13/500 label for compact folder/list rows. */
  labelSize?: 'default' | 'compact';
  /** Renders the label as a `<label>` for this control id. */
  htmlFor?: string;
  /** Overrides the generated label id. */
  labelId?: string;
  /** Overrides the generated help id. */
  helpId?: string;
}

/**
 * One Settings row (`components.md` §5): leading icon, label/help column and a
 * right-aligned control. Changed rows get the 6px brand dot in the 16px gutter
 * and show the saved value instead of the help line.
 */
export function SettingsRow({
  label,
  help,
  icon: Icon,
  iconTone = 'default',
  control,
  changed = false,
  previous,
  compact = false,
  labelSize = 'default',
  htmlFor,
  labelId,
  helpId,
  className,
  ...props
}: SettingsRowProps) {
  const { t } = useI18n();
  const searchMatch = useSettingsSearchMatch(label);
  const generatedId = React.useId();
  const resolvedLabelId = labelId ?? `${generatedId}-label`;
  const resolvedHelpId = helpId ?? `${generatedId}-help`;
  const changedId = `${generatedId}-changed`;
  const showPrevious = changed && previous !== undefined;
  const hasHelp = help !== undefined || showPrevious;
  const describedBy =
    [hasHelp ? resolvedHelpId : null, changed ? changedId : null]
      .filter(Boolean)
      .join(' ') || undefined;
  const controlIds: SettingsRowControlIds = {
    labelId: resolvedLabelId,
    helpId: resolvedHelpId,
    changedId,
    describedBy,
  };

  const labelClassName = cx(
    'text-text-primary',
    labelSize === 'compact'
      ? 'text-body font-medium'
      : 'text-sm font-medium',
    searchMatch &&
      'underline decoration-brand decoration-2 underline-offset-[3px]',
  );

  const labelContent = htmlFor ? (
    <label id={resolvedLabelId} htmlFor={htmlFor} className={labelClassName}>
      {label}
    </label>
  ) : (
    <span id={resolvedLabelId} className={labelClassName}>
      {label}
    </span>
  );

  return (
    <div
      {...props}
      data-changed={changed || undefined}
      data-search-match={searchMatch ? 'true' : undefined}
      className={cx(
        'relative flex flex-wrap items-center border-t border-border-subtle px-md',
        compact ? 'gap-row py-sm' : 'gap-md py-row',
        className,
      )}
    >
      {changed ? (
        <span
          aria-hidden="true"
          data-testid="settings-row-changed-dot"
          className="absolute left-[6px] top-1/2 h-[6px] w-[6px] -translate-y-1/2 rounded-full bg-brand"
        />
      ) : null}
      {Icon ? (
        <Icon
          size={18}
          stroke={2}
          aria-hidden="true"
          className={cx(
            'shrink-0',
            iconTone === 'warning' ? 'text-warning' : 'text-text-secondary',
          )}
        />
      ) : null}
      <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
        {labelContent}
        {hasHelp ? (
          <p
            id={resolvedHelpId}
            className="max-w-settings-copy text-xs leading-note text-text-dimmed"
          >
            {showPrevious
              ? t('settings.common.before', { value: previous })
              : help}
          </p>
        ) : null}
        {changed ? (
          <span id={changedId} className="sr-only">
            {t('settings.common.changed')}
          </span>
        ) : null}
      </div>
      {control ? (
        <div className="ml-auto flex shrink-0 items-center gap-xs">
          {typeof control === 'function' ? control(controlIds) : control}
        </div>
      ) : null}
    </div>
  );
}
