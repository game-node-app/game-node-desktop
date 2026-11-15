import { forwardRef } from 'react';
import {
  Switch as MantineSwitch,
  type SwitchProps as MantineSwitchProps,
  type SwitchStylesNames,
} from '@mantine/core';
import { mergeClassNames } from './cx';

export type SwitchProps = MantineSwitchProps;

/**
 * Mantine Switch restyled to the 34x20 redesign track. The on color is the
 * brand token; the off track reuses the Settings toggle token so both surfaces
 * stay visually consistent.
 */
const switchClassNames: Partial<Record<SwitchStylesNames, string>> = {
  root: [
    '[--switch-height:20px]',
    '[--switch-width:34px]',
    '[--switch-thumb-size:16px]',
    '[--switch-radius:9999px]',
    '[--switch-track-label-padding:2px]',
    '[--switch-color:var(--color-brand)]',
    '[--switch-bg:var(--settings-toggle-track-off)]',
  ].join(' '),
  input: 'peer',
  track:
    '[--switch-bg:var(--settings-toggle-track-off)] peer-checked:[--switch-thumb-bg:#FFFFFF] [--switch-thumb-bg:var(--color-text-muted)]',
  label: 'text-[13px] font-normal text-text-default',
  description: 'text-xs text-text-dimmed',
};

export const Switch = forwardRef<HTMLInputElement, SwitchProps>(function Switch(
  { classNames, withThumbIndicator = false, ...props },
  ref,
) {
  return (
    <MantineSwitch
      ref={ref}
      withThumbIndicator={withThumbIndicator}
      classNames={mergeClassNames(
        switchClassNames,
        classNames as Partial<Record<SwitchStylesNames, string>> | undefined,
      )}
      {...props}
    />
  );
});
