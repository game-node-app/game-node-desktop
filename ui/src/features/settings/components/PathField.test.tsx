import React from 'react';
import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderUiWithI18n } from '../../../components/ui/testUtils';
import { PathField } from './PathField';

function Harness({
  initialValue = 'E:\\Backups\\GameNode',
  onBrowse,
  changed = false,
  error,
}: {
  initialValue?: string;
  onBrowse?: () => void;
  changed?: boolean;
  error?: React.ReactNode;
}) {
  const [value, setValue] = React.useState(initialValue);
  return (
    <PathField
      value={value}
      onChange={setValue}
      onBrowse={onBrowse}
      changed={changed}
      error={error}
      aria-label="Backup destination"
    />
  );
}

describe('PathField', () => {
  it('renders an editable path input with the full path as its tooltip', async () => {
    renderUiWithI18n(<Harness />);

    const input = screen.getByRole('textbox', { name: 'Backup destination' });
    expect(input).toHaveValue('E:\\Backups\\GameNode');
    expect(input).toHaveAttribute('title', 'E:\\Backups\\GameNode');
    expect(input.parentElement).toHaveClass(
      'h-[32px]',
      'w-[230px]',
      'rounded-control',
      'bg-canvas',
      'focus-within:border-brand',
    );

    await userEvent.clear(input);
    await userEvent.type(input, 'D:\\Saves');
    expect(input).toHaveValue('D:\\Saves');
  });

  it('shows the brand border while the path is changed', () => {
    renderUiWithI18n(<Harness changed />);

    const input = screen.getByRole('textbox', { name: 'Backup destination' });
    expect(input.parentElement).toHaveClass('border-brand');
    expect(input.parentElement).not.toHaveClass('border-border-subtle');
  });

  it('opens the folder picker from the secondary button', async () => {
    const onBrowse = vi.fn();
    renderUiWithI18n(<Harness onBrowse={onBrowse} />);

    await userEvent.click(
      screen.getByRole('button', { name: 'Choose folder…' }),
    );
    expect(onBrowse).toHaveBeenCalledTimes(1);
  });

  it('renders a validation error below the field', () => {
    renderUiWithI18n(<Harness error="This folder cannot be used." />);

    expect(screen.getByRole('alert')).toHaveTextContent(
      'This folder cannot be used.',
    );
    expect(
      screen.getByRole('textbox', { name: 'Backup destination' }),
    ).toHaveAttribute('aria-invalid', 'true');
  });
});
