import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconStarFilled } from '@tabler/icons-react';
import { Pill, PILL_TONE_CLASSES } from './Pill';
import { renderUi } from './testUtils';

describe('Pill', () => {
  it('renders the five tones with the redesign colors', () => {
    const tones = ['neutral', 'brand', 'success', 'warning', 'danger'] as const;
    const { rerender } = renderUi(<Pill tone="neutral">Neutral</Pill>);

    for (const tone of tones) {
      rerender(<Pill tone={tone}>{tone}</Pill>);
      expect(screen.getByText(tone).parentElement).toHaveClass(
        ...PILL_TONE_CLASSES[tone].split(' '),
      );
    }
  });

  it('renders an optional 12px icon and the 18px pill height', () => {
    renderUi(<Pill icon={IconStarFilled}>Favorite</Pill>);
    const pill = screen.getByText('Favorite').parentElement;
    expect(pill).toHaveClass('h-[18px]');
    expect(pill?.querySelector('svg')).toHaveAttribute('width', '12');
  });

  it('becomes a button when clickable', async () => {
    const onClick = vi.fn();
    renderUi(
      <Pill tone="neutral" onClick={onClick} aria-label="New collection">
        +
      </Pill>,
    );
    const button = screen.getByRole('button', { name: 'New collection' });
    expect(button).toHaveClass('hover:bg-surface');
    await userEvent.click(button);
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
