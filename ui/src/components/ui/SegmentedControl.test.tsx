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

  it('defaults to 6px segment padding for the library', () => {
    renderUi(
      <SegmentedControl
        aria-label="View"
        data={[
          { value: 'grid', label: 'Grid' },
          { value: 'list', label: 'List' },
        ]}
      />,
    );
    expect(labelFor('Grid')).toHaveClass('px-[6px]');
  });

  it('supports regular and filter segment paddings for Settings', () => {
    renderUi(
      <SegmentedControl
        aria-label="Playtime source"
        segmentPadding="regular"
        data={[
          { value: 'store', label: 'Store' },
          { value: 'internal', label: 'Internal' },
        ]}
      />,
    );
    expect(labelFor('Store')).toHaveClass('px-[12px]');
  });

  it('uses 10px segment padding for the Plugins origin filter', () => {
    renderUi(
      <SegmentedControl
        aria-label="Plugin origin"
        segmentPadding="filter"
        data={[
          { value: 'all', label: 'All' },
          { value: 'official', label: 'Official' },
        ]}
      />,
    );
    expect(labelFor('All')).toHaveClass('px-[10px]');
  });
});

function labelFor(name: string): HTMLElement | null {
  const radio = screen.getByRole('radio', { name });
  return document.querySelector(`label[for="${radio.id}"]`);
}
