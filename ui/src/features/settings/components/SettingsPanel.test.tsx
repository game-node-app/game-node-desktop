import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Button } from '../../../components/ui/Button';
import { renderUi } from '../../../components/ui/testUtils';
import { SettingsPanel } from './SettingsPanel';

describe('SettingsPanel', () => {
  it('renders the title and description as a labelled region', () => {
    renderUi(
      <SettingsPanel
        title="Account & sync"
        description="Your GameNode account and what this PC sends to it."
      >
        <div>Panel content</div>
      </SettingsPanel>,
    );

    const region = screen.getByRole('region', { name: 'Account & sync' });
    expect(region).toBeInTheDocument();
    expect(region).toHaveClass('flex', 'flex-col', 'gap-lg');

    const title = screen.getByRole('heading', {
      level: 2,
      name: 'Account & sync',
    });
    expect(title).toHaveClass(
      'text-heading',
      'font-semibold',
      'text-text-primary',
    );
    expect(region).toHaveAttribute('aria-labelledby', title.id);
    expect(
      screen.getByText(
        'Your GameNode account and what this PC sends to it.',
      ),
    ).toHaveClass('text-body', 'text-text-secondary');
    expect(screen.getByText('Panel content')).toBeInTheDocument();
  });

  it('renders a trailing action in the header row variant', () => {
    renderUi(
      <SettingsPanel
        title="Stores"
        description="Store accounts connected to this PC."
        action={
          <Button size="sm" variant="secondary">
            Rescan
          </Button>
        }
      />,
    );

    const heading = screen.getByRole('heading', { level: 2, name: 'Stores' });
    expect(heading.closest('header')).toHaveClass(
      'flex',
      'items-end',
      'gap-sm',
    );
    expect(screen.getByRole('button', { name: 'Rescan' })).toBeInTheDocument();
  });

  it('uses the column header variant without an action', () => {
    renderUi(<SettingsPanel title="About" />);

    const heading = screen.getByRole('heading', { level: 2, name: 'About' });
    expect(heading.closest('header')).toHaveClass(
      'flex',
      'flex-col',
      'gap-[4px]',
    );
  });
});
