import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { renderUiWithI18n } from '../../../components/ui/testUtils';
import { UnsavedChangesBar } from './UnsavedChangesBar';

describe('UnsavedChangesBar', () => {
  it('announces a single changed setting and exposes both actions', async () => {
    const onRevert = vi.fn();
    const onSave = vi.fn();
    renderUiWithI18n(
      <UnsavedChangesBar count={1} onRevert={onRevert} onSave={onSave} />,
    );

    const region = screen.getByRole('region', { name: 'Unsaved changes' });
    expect(region).toHaveAttribute('aria-live', 'polite');
    expect(region).toHaveClass(
      'absolute',
      'inset-x-0',
      'bottom-0',
      'h-[56px]',
      'bg-raised',
      'border-border-floating',
    );
    expect(screen.getByTestId('settings-unsaved-count')).toHaveTextContent(
      '1 setting changed',
    );
    expect(screen.getByText('Unsaved changes')).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Revert' }));
    expect(onRevert).toHaveBeenCalledTimes(1);
    await userEvent.click(
      screen.getByRole('button', { name: 'Save changes' }),
    );
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it('pluralizes the changed count', () => {
    renderUiWithI18n(
      <UnsavedChangesBar count={3} onRevert={vi.fn()} onSave={vi.fn()} />,
    );

    expect(screen.getByTestId('settings-unsaved-count')).toHaveTextContent(
      '3 settings changed',
    );
  });

  it('shows the save spinner and blocks actions while saving', () => {
    renderUiWithI18n(
      <UnsavedChangesBar
        count={2}
        saving
        onRevert={vi.fn()}
        onSave={vi.fn()}
      />,
    );

    const save = screen.getByRole('button', { name: 'Save changes' });
    expect(save).toBeDisabled();
    expect(save).toHaveAttribute('aria-busy', 'true');
    expect(screen.getByRole('button', { name: 'Revert' })).toBeDisabled();
  });
});
