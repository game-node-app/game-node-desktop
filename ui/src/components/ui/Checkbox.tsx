import { forwardRef } from 'react';
import {
  Checkbox as MantineCheckbox,
  type CheckboxProps as MantineCheckboxProps,
  type CheckboxStylesNames,
} from '@mantine/core';
import { mergeClassNames } from './cx';

export interface CheckboxProps extends MantineCheckboxProps {
  /** Exposed as `aria-checked="mixed"` while keeping the native indeterminate state. */
  indeterminate?: boolean;
}

const checkboxClassNames: Partial<Record<CheckboxStylesNames, string>> = {
  root: 'inline-flex',
  inner: 'items-center',
  input: [
    'h-[16px] w-[16px] cursor-pointer rounded-sm border border-border-strong bg-canvas',
    'transition-colors checked:border-brand checked:bg-brand',
    'data-[indeterminate]:border-brand data-[indeterminate]:bg-brand',
    'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand',
    'focus-visible:ring-offset-2 focus-visible:ring-offset-body',
    'disabled:cursor-not-allowed',
  ].join(' '),
  icon: 'text-white',
  label: 'text-[13px] font-normal text-text-default',
};

export const Checkbox = forwardRef<HTMLInputElement, CheckboxProps>(function Checkbox(
  { classNames, indeterminate, ...props },
  ref,
) {
  return (
    <MantineCheckbox
      ref={ref}
      size="xs"
      radius={4}
      color="brand"
      indeterminate={indeterminate}
      aria-checked={indeterminate ? 'mixed' : undefined}
      classNames={mergeClassNames(
        checkboxClassNames,
        classNames as Partial<Record<CheckboxStylesNames, string>> | undefined,
      )}
      {...props}
    />
  );
});
