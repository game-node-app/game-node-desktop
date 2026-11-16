import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { IconBookmark, IconDeviceGamepad2, IconTrophy } from '@tabler/icons-react';
import { OptionCard, OptionCardGroup } from './OptionCard';
import { Pill } from './Pill';
import { renderUi } from './testUtils';

function StatusCards() {
  const [value, setValue] = useState('playing');
  return (
    <OptionCardGroup
      aria-label="Status"
      value={value}
      onChange={setValue}
      className="grid grid-cols-3 gap-compact"
    >
      <OptionCard value="playing" title="Playing" icon={IconDeviceGamepad2} />
      <OptionCard value="finished" title="Finished" icon={IconTrophy} />
      <OptionCard value="planned" title="Planned" icon={IconBookmark} />
    </OptionCardGroup>
  );
}

describe('OptionCard', () => {
  it('renders the unselected status card classes and radio semantics', () => {
    renderUi(
      <OptionCard value="playing" title="Playing" icon={IconDeviceGamepad2} />,
    );

    const card = screen.getByRole('radio', { name: 'Playing' });
    expect(card).toHaveClass(
      'flex-col',
      'items-start',
      'gap-[6px]',
      'p-[10px]',
      'rounded-md',
      'border-border-subtle',
      'bg-surface',
    );
    expect(card).toHaveAttribute('aria-checked', 'false');
    expect(card.querySelector('svg')).toHaveClass('text-text-secondary');
    expect(screen.getByText('Playing')).toHaveClass(
      'text-body',
      'font-normal',
      'text-text-default',
    );
  });

  it('renders the selected status card classes', () => {
    renderUi(
      <OptionCard
        value="playing"
        title="Playing"
        icon={IconDeviceGamepad2}
        selected
      />,
    );

    const card = screen.getByRole('radio', { name: 'Playing' });
    expect(card).toHaveClass(
      'border-brand',
      'bg-accent-subtle',
    );
    expect(card).toHaveAttribute('aria-checked', 'true');
    expect(card.querySelector('svg')).toHaveClass('text-brand');
    expect(screen.getByText('Playing')).toHaveClass(
      'font-semibold',
      'text-text-primary',
    );
  });

  it('renders a match row as a listbox option with cover, meta and pill', () => {
    renderUi(
      <OptionCard
        value="1"
        variant="row"
        title="Hollow Mountain: Deep Delve"
        meta="2024 · PC, Switch"
        leading={<span data-testid="cover" />}
        trailing={<Pill tone="success">96%</Pill>}
        selected
      />,
    );

    const option = screen.getByRole('option', {
      name: /Hollow Mountain: Deep Delve/,
    });
    expect(option).toHaveAttribute('aria-selected', 'true');
    expect(option).toHaveClass(
      'min-h-[66px]',
      'flex-row',
      'items-center',
      'gap-[10px]',
      'p-compact',
      'border-brand',
      'bg-accent-subtle',
    );
    expect(screen.getByTestId('cover')).toBeInTheDocument();
    expect(screen.getByText('2024 · PC, Switch')).toHaveClass(
      'text-caption',
      'text-text-dimmed',
    );
    expect(screen.getByText('96%')).toBeInTheDocument();
  });

  it('marks an unselected match row with aria-selected=false', () => {
    renderUi(
      <OptionCard value="2" variant="row" title="Hollow Mountain" />,
    );

    const option = screen.getByRole('option', { name: 'Hollow Mountain' });
    expect(option).toHaveAttribute('aria-selected', 'false');
    expect(option).toHaveClass('border-border-subtle');
  });

  it('moves selection and focus with the arrow keys', async () => {
    const user = userEvent.setup();
    renderUi(<StatusCards />);

    const playing = screen.getByRole('radio', { name: 'Playing' });
    expect(playing).toHaveAttribute('tabindex', '0');
    expect(screen.getByRole('radio', { name: 'Finished' })).toHaveAttribute(
      'tabindex',
      '-1',
    );

    playing.focus();
    await user.keyboard('{ArrowRight}');

    const finished = screen.getByRole('radio', { name: 'Finished' });
    await waitFor(() => expect(finished).toHaveAttribute('aria-checked', 'true'));
    expect(finished).toHaveFocus();
    expect(screen.getByRole('radio', { name: 'Playing' })).toHaveAttribute(
      'aria-checked',
      'false',
    );
  });

  it('supports Home and End', async () => {
    const user = userEvent.setup();
    renderUi(<StatusCards />);

    screen.getByRole('radio', { name: 'Playing' }).focus();
    await user.keyboard('{End}');
    await waitFor(() =>
      expect(screen.getByRole('radio', { name: 'Planned' })).toHaveAttribute(
        'aria-checked',
        'true',
      ),
    );

    await user.keyboard('{Home}');
    await waitFor(() =>
      expect(screen.getByRole('radio', { name: 'Playing' })).toHaveAttribute(
        'aria-checked',
        'true',
      ),
    );
  });

  it('selects through a click and calls the caller handler', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const onClick = vi.fn();
    renderUi(
      <OptionCardGroup aria-label="Status" value="playing" onChange={onChange}>
        <OptionCard
          value="finished"
          title="Finished"
          icon={IconTrophy}
          onClick={onClick}
        />
      </OptionCardGroup>,
    );

    await user.click(screen.getByRole('radio', { name: 'Finished' }));
    expect(onClick).toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenCalledWith('finished');
  });
});
