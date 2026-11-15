import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconBrandSteam } from '@tabler/icons-react';
import { GroupHeader } from './GroupHeader';
import { renderUi } from './testUtils';

describe('GroupHeader', () => {
  it('exposes aria-expanded and toggles the group', async () => {
    const onToggle = vi.fn();
    const { rerender } = renderUi(
      <GroupHeader label="Steam" count={164} collapsed={false} onToggle={onToggle} />,
    );
    const header = screen.getByRole('button', { name: /Steam/ });
    expect(header).toHaveAttribute('aria-expanded', 'true');
    expect(header).toHaveClass('h-[36px]');
    await userEvent.click(header);
    expect(onToggle).toHaveBeenCalledTimes(1);

    rerender(
      <GroupHeader label="Steam" count={164} collapsed onToggle={onToggle} />,
    );
    expect(screen.getByRole('button', { name: /Steam/ })).toHaveAttribute(
      'aria-expanded',
      'false',
    );
  });

  it('renders the localized count label, group icon and rule', () => {
    renderUi(
      <GroupHeader
        label="Steam"
        count={164}
        countLabel="164 games"
        icon={IconBrandSteam}
        collapsed={false}
        onToggle={() => undefined}
      />,
    );
    expect(screen.getByText('164 games')).toHaveClass('text-text-dimmed');
    expect(
      screen.getByRole('button', { name: /Steam/ }).querySelectorAll('svg'),
    ).toHaveLength(2);
  });
});
