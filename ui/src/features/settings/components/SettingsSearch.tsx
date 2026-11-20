import React from 'react';
import { IconSearch } from '@tabler/icons-react';
import { cx } from '../../../components/ui/cx';
import { useI18n } from '../../../i18n';

export interface SettingsSearchProps
  extends Omit<
    React.InputHTMLAttributes<HTMLInputElement>,
    'value' | 'onChange' | 'type' | 'className' | 'placeholder' | 'aria-label'
  > {
  value: string;
  onChange: (value: string) => void;
  /** Wrapper classes (the 32px bordered field). */
  className?: string;
  inputClassName?: string;
  /** Defaults to "Search settings"; also used as the accessible name. */
  placeholder?: string;
}

/**
 * Navigation search field (`components.md` §2). Escape clears a non-empty
 * query and stops there so the modal only closes on the next Escape.
 */
export const SettingsSearch = React.forwardRef<
  HTMLInputElement,
  SettingsSearchProps
>(function SettingsSearch(
  {
    value,
    onChange,
    className,
    inputClassName,
    placeholder,
    onKeyDown,
    ...props
  },
  ref,
) {
  const { t } = useI18n();
  const label = placeholder ?? t('settings.shell.searchPlaceholder');

  return (
    <div
      className={cx(
        'flex h-[32px] items-center gap-compact rounded-control border border-border-subtle bg-canvas px-xs focus-within:border-brand',
        className,
      )}
    >
      <IconSearch
        size={14}
        stroke={2}
        aria-hidden="true"
        className="shrink-0 text-text-dimmed"
      />
      <input
        {...props}
        ref={ref}
        type="search"
        value={value}
        placeholder={label}
        aria-label={label}
        onChange={(event) => onChange(event.currentTarget.value)}
        onKeyDown={(event) => {
          onKeyDown?.(event);
          if (event.defaultPrevented) return;
          if (event.key === 'Escape' && value !== '') {
            event.preventDefault();
            event.stopPropagation();
            onChange('');
          }
        }}
        className={cx(
          'min-w-0 flex-1 bg-transparent text-xs text-text-primary outline-none placeholder:text-text-dimmed',
          inputClassName,
        )}
      />
    </div>
  );
});
