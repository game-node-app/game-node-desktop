import React, {
  createContext,
  forwardRef,
  useContext,
  useId,
  useState,
} from 'react';
import {
  Radio as MantineRadio,
  type RadioProps as MantineRadioProps,
  type RadioStylesNames,
} from '@mantine/core';
import { cx, mergeClassNames } from './cx';

const radioClassNames: Partial<Record<RadioStylesNames, string>> = {
  root: 'inline-flex',
  body: 'items-center',
  inner: 'items-center',
  radio: cx(
    'h-[16px] w-[16px] rounded-full border-[1.5px] border-text-muted bg-transparent',
    'checked:border-[5px] checked:border-brand checked:bg-transparent',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand',
    'focus-visible:ring-offset-2 focus-visible:ring-offset-body',
    'disabled:cursor-not-allowed',
  ),
  // The 5px brand border is the selected state; the filled center dot is not.
  icon: 'hidden',
  label: 'text-body font-normal text-text-default',
  description: 'text-xs leading-note text-text-dimmed',
};

export interface RadioProps extends MantineRadioProps {
  classNames?: Partial<Record<RadioStylesNames, string>>;
}

interface RadioGroupContextValue {
  name: string;
  value: string | null;
  select: (value: string) => void;
  disabled?: boolean;
  readOnly?: boolean;
}

const RadioGroupContext = createContext<RadioGroupContextValue | null>(null);

/** Internal hook shared by `Radio` and `RadioCard`. */
export function useRadioGroupContext(): RadioGroupContextValue | null {
  return useContext(RadioGroupContext);
}

export interface RadioGroupProps {
  name?: string;
  value?: string | null;
  defaultValue?: string | null;
  onChange?: (value: string) => void;
  disabled?: boolean;
  readOnly?: boolean;
  orientation?: 'horizontal' | 'vertical';
  'aria-label'?: string;
  'aria-labelledby'?: string;
  className?: string;
  children: React.ReactNode;
}

/**
 * Radiogroup wrapper with explicit arrow-key navigation from `components.md`
 * §4. Native radio groups already move with the arrows in a browser; handling
 * the keys here as well keeps the behavior deterministic for jsdom tests and
 * for cards whose radios are wrapped in labels.
 */
export function RadioGroup({
  name,
  value,
  defaultValue,
  onChange,
  disabled = false,
  readOnly = false,
  orientation = 'vertical',
  'aria-label': ariaLabel,
  'aria-labelledby': ariaLabelledBy,
  className,
  children,
}: RadioGroupProps) {
  const generatedName = useId();
  const groupName = name ?? generatedName;
  const [internalValue, setInternalValue] = useState<string | null>(
    defaultValue ?? null,
  );
  const currentValue = value !== undefined ? value : internalValue;

  const select = (next: string) => {
    if (disabled || readOnly) return;
    if (value === undefined) setInternalValue(next);
    onChange?.(next);
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (
      event.key !== 'ArrowDown' &&
      event.key !== 'ArrowUp' &&
      event.key !== 'ArrowLeft' &&
      event.key !== 'ArrowRight' &&
      event.key !== 'Home' &&
      event.key !== 'End'
    ) {
      return;
    }

    const radios = Array.from(
      event.currentTarget.querySelectorAll<HTMLInputElement>(
        'input[type="radio"][role="radio"]',
      ),
    ).filter((radio) => radio.name === groupName && !radio.disabled);
    if (radios.length === 0) return;

    const target = event.target as Node;
    const currentIndex = radios.findIndex(
      (radio) => radio === target || radio.contains(target),
    );
    if (currentIndex === -1) return;

    event.preventDefault();

    let nextIndex: number;
    if (event.key === 'Home') {
      nextIndex = 0;
    } else if (event.key === 'End') {
      nextIndex = radios.length - 1;
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowRight') {
      nextIndex = (currentIndex + 1) % radios.length;
    } else {
      nextIndex = (currentIndex - 1 + radios.length) % radios.length;
    }

    const next = radios[nextIndex];
    const nextValue = next.value;
    if (nextValue) select(nextValue);
    next.focus();
  };

  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      aria-labelledby={ariaLabelledBy}
      aria-orientation={orientation}
      className={className}
      onKeyDown={handleKeyDown}
    >
      <RadioGroupContext.Provider
        value={{
          name: groupName,
          value: currentValue,
          select,
          disabled,
          readOnly,
        }}
      >
        {children}
      </RadioGroupContext.Provider>
    </div>
  );
}

/**
 * Mantine `Radio` restyled to the 16px ring from `components.md` §4: a 1.5px
 * muted outline when unselected and a 5px brand ring with a transparent center
 * when selected. Inside a `RadioGroup` it takes its name, checked state and
 * change handler from the group.
 */
export const Radio = forwardRef<HTMLInputElement, RadioProps>(function Radio(
  { classNames, value, checked, onChange, name, disabled, ...props },
  ref,
) {
  const group = useRadioGroupContext();
  const isChecked = group
    ? value !== undefined && group.value === String(value)
    : checked;

  return (
    <MantineRadio
      ref={ref}
      size="xs"
      color="brand"
      value={value}
      name={group?.name ?? name}
      checked={isChecked}
      disabled={disabled ?? group?.disabled}
      onChange={(event) => {
        if (group) {
          if (value !== undefined) group.select(String(value));
          return;
        }
        onChange?.(event);
      }}
      classNames={mergeClassNames(
        radioClassNames,
        classNames as Partial<Record<RadioStylesNames, string>> | undefined,
      )}
      {...props}
    />
  );
});
