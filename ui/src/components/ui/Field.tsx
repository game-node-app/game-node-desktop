import React, { createContext, useContext, useId } from 'react';
import { IconInfoCircle } from '@tabler/icons-react';
import { cx } from './cx';

interface FieldControlContextValue {
  controlId: string;
  labelId?: string;
  describedBy?: string;
}

const FieldControlContext = createContext<FieldControlContextValue | null>(null);

/**
 * Wiring shared by the controls a `Field` wraps: the generated id for the real
 * `<label>`, the label id for controls that must use `aria-labelledby`, and the
 * ids of the helper/warning messages. Controls also accept explicit ids and
 * `aria-describedby` values, which take precedence.
 */
export function useFieldControl(): FieldControlContextValue | null {
  return useContext(FieldControlContext);
}

export interface FieldProps {
  label?: React.ReactNode;
  /** Explicit control id; otherwise the field generates one for the label. */
  id?: string;
  /** Muted helper line under the control, per `components.md` §3. */
  helper?: React.ReactNode;
  /** Warning helper line with the 14px info icon. */
  warning?: React.ReactNode;
  className?: string;
  children: React.ReactNode;
}

/**
 * Field label + control + helper stack from `components.md` §3. The label is a
 * real `<label>` bound to the control through the context id, so a wrapped
 * `TextInput`, `TextArea` or `SelectField` is labelled without extra wiring.
 */
export function Field({
  label,
  id,
  helper,
  warning,
  className,
  children,
}: FieldProps) {
  const generatedId = useId();
  const controlId = id ?? generatedId;
  const labelId = label !== undefined ? `${controlId}-label` : undefined;
  const helperId = helper !== undefined ? `${controlId}-helper` : undefined;
  const warningId = warning !== undefined ? `${controlId}-warning` : undefined;
  const describedBy =
    [helperId, warningId].filter(Boolean).join(' ') || undefined;

  return (
    <FieldControlContext.Provider
      value={{ controlId, labelId, describedBy }}
    >
      <div className={cx('flex flex-col gap-[6px]', className)}>
        {label !== undefined ? (
          <label
            id={labelId}
            htmlFor={controlId}
            className="text-xs font-medium text-text-secondary"
          >
            {label}
          </label>
        ) : null}
        {children}
        {helper !== undefined ? (
          <span
            id={helperId}
            className="flex items-center gap-compact text-xs text-text-dimmed"
          >
            {helper}
          </span>
        ) : null}
        {warning !== undefined ? (
          <span
            id={warningId}
            className="flex items-center gap-compact text-xs text-warning"
          >
            <IconInfoCircle size={14} stroke={2} aria-hidden="true" />
            {warning}
          </span>
        ) : null}
      </div>
    </FieldControlContext.Provider>
  );
}
