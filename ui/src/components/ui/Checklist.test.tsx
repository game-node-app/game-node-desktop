import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Checklist } from './Checklist';
import { renderUi } from './testUtils';

const items = [
  { kind: 'positive' as const, label: 'Playtime from stores' },
  { kind: 'negative' as const, label: 'Save files (stay on this PC)' },
  { kind: 'warning' as const, label: 'Title 74% similar' },
];

describe('Checklist', () => {
  it('renders one icon style per kind', () => {
    renderUi(<Checklist items={items} />);

    const positive = screen.getByText('Playtime from stores').parentElement;
    const negative = screen
      .getByText('Save files (stay on this PC)')
      .parentElement;
    const warning = screen.getByText('Title 74% similar').parentElement;

    expect(positive?.querySelector('svg')).toHaveClass('text-success');
    expect(positive).toHaveClass('text-text-default');
    expect(negative?.querySelector('svg')).toHaveClass('text-text-muted');
    expect(negative).toHaveClass('text-text-dimmed');
    expect(warning?.querySelector('svg')).toHaveClass('text-warning');
    expect(warning).toHaveClass('text-text-default');

    for (const row of [positive, negative, warning]) {
      expect(row).toHaveClass('flex', 'items-center', 'gap-compact', 'text-xs');
      expect(row?.querySelector('svg')).toHaveAttribute('width', '14');
      expect(row?.querySelector('svg')).toHaveAttribute('aria-hidden', 'true');
    }
  });

  it('renders the plain list with the 8px row gap', () => {
    renderUi(<Checklist items={items} />);
    expect(screen.getByRole('list')).toHaveClass('flex', 'flex-col', 'gap-compact');
    expect(screen.getAllByRole('listitem')).toHaveLength(3);
  });

  it('renders the inset list surface used by the sync explanation', () => {
    renderUi(<Checklist variant="inset" items={items} />);
    expect(screen.getByRole('list')).toHaveClass(
      'rounded-md',
      'border',
      'border-border-subtle',
      'bg-body',
      'p-sm',
    );
  });

  it('renders the bullet variant with brand dots instead of icons', () => {
    renderUi(
      <Checklist
        variant="bullet"
        items={[{ kind: 'positive', label: 'Backup destination' }]}
      />,
    );

    const list = screen.getByRole('list');
    expect(list).toHaveClass(
      'gap-[4px]',
      'rounded-md',
      'border',
      'border-border-subtle',
      'bg-body',
      'p-[10px]',
    );
    const row = screen.getByText('Backup destination').parentElement;
    expect(row).toHaveClass('text-text-default');
    expect(row?.querySelector('svg')).toBeNull();
    expect(row?.querySelector('span[aria-hidden="true"]')).toHaveClass(
      'h-[6px]',
      'w-[6px]',
      'rounded-full',
      'bg-brand',
    );
  });
});
