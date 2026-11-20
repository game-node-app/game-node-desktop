import React, { forwardRef } from 'react';
import { Combobox, useCombobox } from '@mantine/core';
import { IconCheck, IconChevronDown } from '@tabler/icons-react';
import { cx } from './cx';
import { useFieldControl } from './Field';
import { menuClassNames } from './menuClasses';

export type SelectFieldSize = 'md' | 'compact';

export interface SelectFieldOption {
  value: string;
  label: React.ReactNode;
  disabled?: boolean;
}

export interface SelectFieldProps {
  options: SelectFieldOption[];
  value?: string | null;
  onChange?: (value: string) => void;
  placeholder?: string;
  /** `md` is the 34px dialog field; `compact` is the 30px table select (D08). */
  size?: SelectFieldSize;
  disabled?: boolean;
  readOnly?: boolean;
  id?: string;
  className?: string;
  'aria-label'?: string;
  'aria-labelledby'?: string;
  'aria-describedby'?: string;
  'data-testid'?: string;
}

/**
 * Select-style field from `components.md` §3/§10. The box is the same 34px
 * surface as `TextInput` with a trailing chevron; the dropdown is a Mantine
 * `Combobox` styled with the shared `menuClasses.ts` surface so selects and
 * menus cannot drift apart. Arrow keys and Enter are handled by Mantine's
 * combobox target, so the control is keyboard operable without custom keys.
 */
export const SelectField = forwardRef<HTMLButtonElement, SelectFieldProps>(
  function SelectField(
    {
      options,
      value,
      onChange,
      placeholder,
      size = 'md',
      disabled = false,
      readOnly = false,
      id,
      className,
      'aria-label': ariaLabel,
      'aria-labelledby': ariaLabelledBy,
      'aria-describedby': ariaDescribedBy,
      'data-testid': testId,
    },
    ref,
  ) {
    const field = useFieldControl();
    const controlId = id ?? field?.controlId;
    const describedBy =
      cx(ariaDescribedBy, field?.describedBy) || undefined;
    const labelledBy = ariaLabelledBy ?? field?.labelId;

    const combobox = useCombobox({
      onDropdownClose: () => combobox.resetSelectedOption(),
    });

    const selectedOption = options.find((option) => option.value === value);
    const compact = size === 'compact';

    return (
      <Combobox
        store={combobox}
        width="target"
        position="bottom-start"
        withinPortal
        transitionProps={{ duration: 0 }}
        readOnly={readOnly || disabled}
        onOptionSubmit={(optionValue) => {
          onChange?.(String(optionValue));
          combobox.closeDropdown();
        }}
      >
        <Combobox.Target targetType="button" withExpandedAttribute>
          <button
            ref={ref}
            id={controlId}
            type="button"
            disabled={disabled}
            aria-readonly={readOnly || undefined}
            aria-label={ariaLabel}
            aria-labelledby={labelledBy}
            aria-describedby={describedBy}
            data-testid={testId}
            onClick={() => {
              if (disabled || readOnly) return;
              combobox.toggleDropdown();
            }}
            className={cx(
              'flex items-center rounded-control border border-border-subtle bg-surface focus:border-brand focus-visible:border-brand focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50',
              compact
                ? 'h-[30px] gap-[6px] px-compact text-xs'
                : 'h-[34px] gap-compact px-xs text-body',
              className,
            )}
          >
            <span
              className={cx(
                'min-w-0 flex-1 truncate text-left',
                selectedOption
                  ? compact
                    ? 'text-text-default'
                    : 'text-text-primary'
                  : 'text-text-dimmed',
              )}
            >
              {selectedOption ? selectedOption.label : placeholder}
            </span>
            <IconChevronDown
              size={compact ? 12 : 14}
              stroke={2}
              aria-hidden="true"
              className="shrink-0 text-text-dimmed"
            />
          </button>
        </Combobox.Target>

        <Combobox.Dropdown className={menuClassNames.dropdown}>
          <Combobox.Options className="flex flex-col gap-px">
            {options.map((option) => {
              const selected = option.value === value;
              return (
                <Combobox.Option
                  key={option.value}
                  value={option.value}
                  disabled={option.disabled}
                  active={selected}
                  selected={selected}
                  aria-selected={selected}
                  className={menuClassNames.item}
                >
                  <span className={menuClassNames.itemLabel}>
                    {option.label}
                  </span>
                  {selected ? (
                    <IconCheck
                      size={16}
                      stroke={2}
                      aria-hidden="true"
                      className="shrink-0 text-brand"
                    />
                  ) : null}
                </Combobox.Option>
              );
            })}
          </Combobox.Options>
        </Combobox.Dropdown>
      </Combobox>
    );
  },
);
