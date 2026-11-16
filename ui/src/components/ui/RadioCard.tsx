import React, { useId } from 'react';
import { cx } from './cx';
import { Radio, useRadioGroupContext } from './Radio';

export interface RadioCardProps {
  value: string;
  title: React.ReactNode;
  description?: React.ReactNode;
  name?: string;
  checked?: boolean;
  onChange?: React.ChangeEventHandler<HTMLInputElement>;
  disabled?: boolean;
  className?: string;
  'data-testid'?: string;
}

/**
 * Card with a 16px radio and a title/description column from `components.md`
 * §4, used by the remove-game confirmation. Unselected cards sit on `surface`;
 * the selected one uses the brand border and `accent-subtle`. It can be
 * controlled directly or placed inside a `RadioGroup`.
 */
export function RadioCard({
  value,
  title,
  description,
  name,
  checked,
  onChange,
  disabled,
  className,
  'data-testid': testId,
}: RadioCardProps) {
  const group = useRadioGroupContext();
  const isSelected = group ? group.value === value : Boolean(checked);
  const isDisabled = disabled ?? group?.disabled ?? false;
  const titleId = useId();
  const descriptionId = useId();

  return (
    <label
      data-testid={testId}
      className={cx(
        'flex cursor-pointer items-start gap-sm rounded-md border p-sm transition-colors',
        isSelected
          ? 'border-brand bg-accent-subtle'
          : 'border-border-subtle bg-surface hover:bg-raised',
        isDisabled && 'cursor-not-allowed opacity-50',
        className,
      )}
    >
      <Radio
        value={value}
        name={group?.name ?? name}
        checked={isSelected}
        onChange={group ? undefined : onChange}
        disabled={isDisabled}
        aria-labelledby={titleId}
        aria-describedby={description !== undefined ? descriptionId : undefined}
        className="mt-[1px]"
      />
      <span className="flex min-w-0 flex-col gap-[3px]">
        <span
          id={titleId}
          className="text-body font-semibold text-text-primary"
        >
          {title}
        </span>
        {description !== undefined ? (
          <span
            id={descriptionId}
            className="text-xs leading-note text-text-dimmed"
          >
            {description}
          </span>
        ) : null}
      </span>
    </label>
  );
}
