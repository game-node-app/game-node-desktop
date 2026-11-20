import { describe, expect, it, vi } from 'vitest';
import type { ReactElement } from 'react';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MantineProvider } from '@mantine/core';
import { appTheme } from '../../AppProviders';
import { Field } from './Field';
import { SelectField } from './SelectField';

/** Mantine's `env="test"` skips detached-reference hiding, which jsdom always reports. */
function renderSelect(ui: ReactElement) {
  const wrap = (element: ReactElement) => (
    <MantineProvider theme={appTheme} defaultColorScheme="dark" env="test">
      {element}
    </MantineProvider>
  );
  const result = render(wrap(ui));
  return {
    ...result,
    rerender: (next: ReactElement) => result.rerender(wrap(next)),
  };
}

const options = [
  { value: 'stable', label: 'Stable' },
  { value: 'nightly', label: 'Nightly' },
];

describe('SelectField', () => {
  it('renders the selected option inside the field box and binds the label', () => {
    renderSelect(
      <Field label="Channel">
        <SelectField
          options={options}
          value="stable"
          placeholder="Choose a channel"
        />
      </Field>,
    );

    const select = screen.getByRole('combobox', { name: 'Channel' });
    expect(select).toHaveTextContent('Stable');
    expect(select).toHaveClass(
      'h-[34px]',
      'items-center',
      'gap-compact',
      'rounded-control',
      'border',
      'border-border-subtle',
      'bg-surface',
      'px-xs',
      'text-body',
    );
    expect(select.querySelector('svg')).toHaveAttribute('width', '14');
  });

  it('shows the placeholder in the dimmed treatment when nothing is selected', () => {
    renderSelect(
      <SelectField
        aria-label="Version"
        options={options}
        placeholder="Choose a version"
      />,
    );

    const select = screen.getByRole('combobox', { name: 'Version' });
    const value = screen.getByText('Choose a version');
    expect(select).toContainElement(value);
    expect(value).toHaveClass('text-text-dimmed');
  });

  it('opens the shared menu surface and selects an option with the mouse', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderSelect(
      <SelectField
        aria-label="Channel"
        options={options}
        value="stable"
        onChange={onChange}
      />,
    );

    await user.click(screen.getByRole('combobox', { name: 'Channel' }));
    const nightly = await screen.findByRole('option', { name: 'Nightly' });
    expect(screen.getByRole('listbox').parentElement).toHaveClass(
      'rounded-md',
      'border',
      'border-border-overlay',
      'bg-secondary',
      'shadow-toast',
    );

    await user.click(nightly);
    expect(onChange).toHaveBeenCalledWith('nightly');
    expect(
      screen.getByRole('combobox', { name: 'Channel' }),
    ).toHaveAttribute('aria-expanded', 'false');
  });

  it('supports choosing with the keyboard', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderSelect(
      <SelectField
        aria-label="Channel"
        options={options}
        value="stable"
        onChange={onChange}
      />,
    );

    const select = screen.getByRole('combobox', { name: 'Channel' });
    select.focus();
    await user.keyboard('{Enter}');
    expect(await screen.findByRole('listbox')).toBeInTheDocument();

    await user.keyboard('{ArrowDown}{ArrowDown}{Enter}');
    expect(onChange).toHaveBeenCalledWith('nightly');
    expect(select).toHaveAttribute('aria-expanded', 'false');
  });

  it('does not open while disabled or read-only', async () => {
    const user = userEvent.setup();
    const { rerender } = renderSelect(
      <SelectField
        aria-label="Channel"
        options={options}
        value="stable"
        disabled
      />,
    );

    await user.click(screen.getByRole('combobox', { name: 'Channel' }));
    expect(
      screen.getByRole('combobox', { name: 'Channel' }),
    ).toHaveAttribute('aria-expanded', 'false');

    rerender(
      <SelectField
        aria-label="Channel"
        options={options}
        value="stable"
        readOnly
      />,
    );
    const readOnly = screen.getByRole('combobox', { name: 'Channel' });
    expect(readOnly).toHaveAttribute('aria-readonly', 'true');
    await user.click(readOnly);
    expect(readOnly).toHaveAttribute('aria-expanded', 'false');
  });

  it('renders the compact table variant', () => {
    renderSelect(
      <SelectField
        aria-label="Executable"
        options={[{ value: 'exe', label: 'MossMachine.exe' }]}
        value="exe"
        size="compact"
      />,
    );

    const select = screen.getByRole('combobox', { name: 'Executable' });
    expect(select).toHaveClass('h-[30px]', 'gap-[6px]', 'px-compact', 'text-xs');
    expect(select.querySelector('svg')).toHaveAttribute('width', '12');
  });
});
