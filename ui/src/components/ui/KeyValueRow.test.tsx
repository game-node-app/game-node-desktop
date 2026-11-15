import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { KeyValueRow } from './KeyValueRow';
import { renderUi } from './testUtils';

describe('KeyValueRow', () => {
  it('renders the key, value and bottom border', () => {
    renderUi(<KeyValueRow label="Install folder" value="/games/ember" />);
    expect(screen.getByText('Install folder')).toHaveClass('text-text-secondary');
    expect(screen.getByText('/games/ember')).toHaveClass('text-text-default');
    expect(screen.getByText('Install folder').parentElement).toHaveClass(
      'border-b',
      'border-border-subtle',
    );
  });

  it('renders the localized empty label dimmed and omits the border on last rows', () => {
    renderUi(<KeyValueRow label="Collections" emptyLabel="None" last />);
    expect(screen.getByText('None')).toHaveClass('text-text-dimmed');
    expect(screen.getByText('Collections').parentElement).not.toHaveClass('border-b');
  });

  it('becomes a whole-row button with a chevron and hover feedback', async () => {
    const onClick = vi.fn();
    renderUi(<KeyValueRow label="Achievements" value="12 / 40" onClick={onClick} />);
    const row = screen.getByRole('button', { name: /Achievements/ });
    expect(row).toHaveClass('hover:bg-raised');
    expect(row.querySelector('svg')).not.toBeNull();
    await userEvent.click(row);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it('can show a chevron without being interactive', () => {
    renderUi(<KeyValueRow label="Game tools" chevron />);
    expect(screen.getByText('Game tools')).toBeInTheDocument();
    expect(
      screen.getByText('Game tools').parentElement?.querySelector('svg'),
    ).not.toBeNull();
  });
});
