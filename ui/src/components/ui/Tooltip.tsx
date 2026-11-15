import React from 'react';
import { Tooltip as MantineTooltip, type TooltipProps as MantineTooltipProps } from '@mantine/core';
import { KbdCombo } from './Kbd';

export interface TooltipProps
  extends Omit<MantineTooltipProps, 'label' | 'children' | 'classNames'> {
  /** The visible tooltip text. IconButton passes its required `label` here. */
  label: React.ReactNode;
  /** Optional shortcut rendered after the label as key caps. */
  shortcut?: string[];
  children: React.ReactElement;
}

/**
 * Mantine Tooltip restyled to the redesign tooltip token: 5/8 padding, 6px
 * radius, `--color-tooltip` background, a 12/400 primary label and optional
 * KbdCombo. It opens after 400 ms and on keyboard focus.
 */
export function Tooltip({
  label,
  shortcut,
  children,
  openDelay = 400,
  events = { hover: true, focus: true, touch: true },
  ...props
}: TooltipProps) {
  return (
    <MantineTooltip
      {...props}
      openDelay={openDelay}
      events={events}
      withinPortal
      classNames={{
        tooltip:
          'rounded-control border-0 bg-tooltip px-[8px] py-[5px] text-xs font-normal leading-none text-text-primary shadow-none',
      }}
      label={
        <span className="flex items-center gap-[8px] whitespace-nowrap">
          <span>{label}</span>
          {shortcut && shortcut.length > 0 ? <KbdCombo keys={shortcut} /> : null}
        </span>
      }
    >
      {children}
    </MantineTooltip>
  );
}
