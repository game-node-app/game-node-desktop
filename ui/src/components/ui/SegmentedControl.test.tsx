import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconLayoutGrid, IconLayoutList } from '@tabler/icons-react';
import { SegmentedControl } from './SegmentedControl';
import { renderUi } from './testUtils';

describe('SegmentedControl', () => {
  it('renders labelled options as a radiogroup', async () => {
    const onChange = vi.fn();
    renderUi(
      <SegmentedControl
        aria-label="View"
        value="grid"
        onChange={onChange}
        data={[
          { value: 'grid', label: 'Grid' },
          { value: 'list', label: 'List' },
        ]}
      />,
    );

    expect(screen.getByRole('radiogroup', { name: 'View' })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'Grid' })).toBeChecked();
    await userEvent.click(screen.getByRole('radio', { name: 'List' }));
    expect(onChange).toHaveBeenCalledWith('list');
  });

  it('supports icon-only options with a visually hidden accessible label', () => {
    renderUi(
      <SegmentedControl
        aria-label="View"
        defaultValue="grid"
        data={[
          { value: 'grid', label: 'Grid', icon: IconLayoutGrid, iconOnly: true },
          { value: 'list', label: 'List', icon: IconLayoutList, iconOnly: true },
        ]}
      />,
    );

    expect(screen.getByRole('radio', { name: 'Grid' })).toBeInTheDocument();
    expect(screen.getByRole('radio', { name: 'List' })).toBeInTheDocument();
  });

  it('allows a custom aria label for an icon-only option', () => {
    renderUi(
      <SegmentedControl
        aria-label="View"
        data={[
          {
            value: 'grid',
            label: 'ignored',
            ariaLabel: 'Grid view',
            icon: IconLayoutGrid,
            iconOnly: true,
          },
        ]}
      />,
    );
    expect(screen.getByRole('radio', { name: 'Grid view' })).toBeInTheDocument();
  });

  it('marks disabled options as disabled', () => {
    renderUi(
      <SegmentedControl
        aria-label="View"
        data={[
          { value: 'grid', label: 'Grid' },
          { value: 'list', label: 'List', disabled: true },
        ]}
      />,
    );
    expect(screen.getByRole('radio', { name: 'List' })).toBeDisabled();
  });
});
