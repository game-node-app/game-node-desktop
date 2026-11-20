import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { IconShieldCheck } from '@tabler/icons-react';
import { CALLOUT_TONE_CLASSES, Callout } from './Callout';
import { renderUi } from './testUtils';

describe('Callout', () => {
  it('renders a labelled region, never an alert', () => {
    renderUi(
      <Callout title="Anti-cheat detected (Easy Anti-Cheat)">
        Installing proxy DLLs in online games with anti-cheat can get you banned.
      </Callout>,
    );

    const region = screen.getByRole('region', {
      name: 'Anti-cheat detected (Easy Anti-Cheat)',
    });
    expect(region).not.toHaveAttribute('role', 'alert');
    expect(region).toHaveClass(
      'flex',
      'flex-col',
      'gap-compact',
      'rounded-md',
      'border',
      'p-sm',
      'border-danger',
      'bg-danger-subtle',
    );
    expect(screen.getByText(/Installing proxy DLLs/)).toHaveClass(
      'text-xs',
      'leading-note',
      'text-text-secondary',
    );
  });

  it('applies each tone to the container and the header icon', () => {
    const tones = ['accent', 'success', 'warning', 'danger', 'neutral'] as const;
    const { rerender } = renderUi(<Callout title="Tone">Body</Callout>);

    for (const tone of tones) {
      rerender(
        <Callout tone={tone} title="Tone">
          Body
        </Callout>,
      );
      const region = screen.getByRole('region', { name: 'Tone' });
      expect(region).toHaveClass(
        ...CALLOUT_TONE_CLASSES[tone].container.split(' '),
      );
      expect(region.querySelector('svg')).toHaveClass(
        ...CALLOUT_TONE_CLASSES[tone].icon.split(' '),
      );
    }
  });

  it('renders the header icon at 16px and a custom icon', () => {
    const { rerender } = renderUi(<Callout title="Danger">Body</Callout>);
    let region = screen.getByRole('region', { name: 'Danger' });
    expect(region.querySelector('svg')).toHaveAttribute('width', '16');

    rerender(
      <Callout title="Protected" tone="success" icon={IconShieldCheck}>
        Body
      </Callout>,
    );
    region = screen.getByRole('region', { name: 'Protected' });
    expect(region.querySelector('svg')).toHaveAttribute('width', '16');
  });

  it('renders a confirmation row for the acknowledgement checkbox', () => {
    renderUi(
      <Callout
        title="Anti-cheat detected"
        confirmation={<span>I understand the risk</span>}
      >
        Body
      </Callout>,
    );

    const confirmation = screen.getByText('I understand the risk');
    expect(confirmation.parentElement).toHaveClass(
      'flex',
      'items-center',
      'gap-[10px]',
      'text-xs',
      'text-text-primary',
    );
  });
});
