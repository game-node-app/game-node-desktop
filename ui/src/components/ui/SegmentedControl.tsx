import React from 'react';
import {
  SegmentedControl as MantineSegmentedControl,
  type SegmentedControlProps as MantineSegmentedControlProps,
  type SegmentedControlStylesNames,
} from '@mantine/core';
import type { TablerIcon } from '@tabler/icons-react';
import { mergeClassNames } from './cx';

export interface SegmentedControlOption<Value extends string = string> {
  value: Value;
  /** Visible label. Required for screen readers when `iconOnly` is set. */
  label?: React.ReactNode;
  icon?: TablerIcon;
  /** Renders only the icon, with the label kept as visually hidden text. */
  iconOnly?: boolean;
  /** Overrides the accessible hidden label for icon-only options. */
  ariaLabel?: string;
  disabled?: boolean;
}

export interface SegmentedControlProps<Value extends string = string>
  extends Omit<
    MantineSegmentedControlProps<Value>,
    'data' | 'value' | 'defaultValue' | 'onChange' | 'classNames'
  > {
  data: SegmentedControlOption<Value>[];
  value?: Value;
  defaultValue?: Value;
  onChange?: (value: Value) => void;
  classNames?: Partial<Record<SegmentedControlStylesNames, string>>;
}

const segmentedClassNames: Partial<Record<SegmentedControlStylesNames, string>> = {
  root: 'gap-[2px] rounded-md border border-border-subtle bg-canvas p-[3px]',
  control: 'h-[26px] rounded-segment border-0 bg-transparent',
  label:
    'group flex h-[26px] items-center justify-center gap-[6px] rounded-segment px-[6px] text-xs font-normal text-text-secondary transition-colors data-[active]:font-semibold data-[active]:text-text-primary',
  innerLabel: 'inline-flex items-center justify-center',
  indicator: 'rounded-segment bg-raised',
};

function optionLabel<Value extends string>(
  option: SegmentedControlOption<Value>,
): React.ReactNode {
  const Icon = option.icon;
  if (!Icon) return option.label;

  if (option.iconOnly) {
    return (
      <span className="inline-flex items-center justify-center">
        <Icon
          size={16}
          stroke={2}
          aria-hidden="true"
          className="text-text-dimmed group-data-[active]:text-text-primary"
        />
        <span className="sr-only">{option.ariaLabel ?? option.label}</span>
      </span>
    );
  }

  return (
    <span className="inline-flex items-center gap-1.5">
      <Icon
        size={16}
        stroke={2}
        aria-hidden="true"
        className="text-text-dimmed group-data-[active]:text-text-primary"
      />
      {option.label}
    </span>
  );
}

/** Mantine SegmentedControl restyled to the 26px track and segment tokens. */
export function SegmentedControl<Value extends string = string>({
  data,
  classNames,
  transitionDuration = 0,
  withItemsBorders = false,
  ...props
}: SegmentedControlProps<Value>) {
  return (
    <MantineSegmentedControl
      {...props}
      data={data.map((option) => ({
        value: option.value,
        label: optionLabel(option),
        disabled: option.disabled,
      }))}
      transitionDuration={transitionDuration}
      withItemsBorders={withItemsBorders}
      classNames={mergeClassNames(segmentedClassNames, classNames)}
    />
  );
}
