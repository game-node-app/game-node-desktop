import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { IconSearch } from '@tabler/icons-react';
import { Field } from './Field';
import { TextInput } from './TextInput';
import { renderUi } from './testUtils';

describe('TextInput', () => {
  it('renders the 34px box, focus border and value classes', () => {
    renderUi(<TextInput defaultValue="Hollow Mountain" aria-label="Search" />);

    const input = screen.getByRole('textbox', { name: 'Search' });
    const box = input.parentElement;

    expect(box).toHaveClass(
      'flex',
      'h-[34px]',
      'items-center',
      'gap-compact',
      'rounded-control',
      'border',
      'border-border-subtle',
      'bg-surface',
      'px-xs',
      'focus-within:border-brand',
    );
    expect(input).toHaveClass(
      'min-w-0',
      'flex-1',
      'truncate',
      'bg-transparent',
      'text-body',
      'text-text-primary',
      'outline-none',
    );
  });

  it('binds to a Field label and its helper', () => {
    renderUi(
      <Field label="Target executable" helper="Used to launch the game">
        <TextInput />
      </Field>,
    );

    const input = screen.getByLabelText('Target executable');
    const helper = screen.getByText('Used to launch the game');
    expect(input).toHaveAttribute('aria-describedby', helper.id);
  });

  it('drops the focus border and marks read-only inputs', () => {
    renderUi(<TextInput aria-label="Save path" value="%APPDATA%" readOnly />);

    const input = screen.getByRole('textbox', { name: 'Save path' });
    expect(input).toHaveAttribute('readonly');
    expect(input).toHaveAttribute('aria-readonly', 'true');
    expect(input.parentElement).not.toHaveClass('focus-within:border-brand');
  });

  it('renders a 14px leading icon and the placeholder treatment', () => {
    renderUi(
      <TextInput
        aria-label="Search"
        leadingIcon={IconSearch}
        placeholder="Search GameNode"
      />,
    );

    const input = screen.getByPlaceholderText('Search GameNode');
    expect(input).toHaveClass('placeholder:text-text-dimmed');
    const icon = input.parentElement?.querySelector('svg');
    expect(icon).toHaveAttribute('width', '14');
    expect(icon).toHaveAttribute('aria-hidden', 'true');
  });

  it('merges caller classes on the box and the input', () => {
    renderUi(
      <TextInput
        aria-label="Name"
        className="box-extra"
        inputClassName="input-extra"
      />,
    );

    const input = screen.getByRole('textbox', { name: 'Name' });
    expect(input.parentElement).toHaveClass('box-extra');
    expect(input).toHaveClass('input-extra');
  });
});
