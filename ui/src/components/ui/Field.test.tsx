import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Field } from './Field';
import { TextInput } from './TextInput';
import { renderUi } from './testUtils';

describe('Field', () => {
  it('binds a real label to the wrapped control', () => {
    renderUi(
      <Field label="Search GameNode">
        <TextInput placeholder="Hollow Mountain" />
      </Field>,
    );

    const input = screen.getByLabelText('Search GameNode');
    const label = screen.getByText('Search GameNode');
    expect(label.tagName).toBe('LABEL');
    expect(label).toHaveAttribute('for', input.id);
    expect(input.id).not.toBe('');
  });

  it('uses the 6px stack and the 12/500 label treatment', () => {
    renderUi(
      <Field label="Collection name">
        <TextInput />
      </Field>,
    );

    expect(screen.getByText('Collection name').parentElement).toHaveClass(
      'flex',
      'flex-col',
      'gap-[6px]',
    );
    expect(screen.getByText('Collection name')).toHaveClass(
      'text-xs',
      'font-medium',
      'text-text-secondary',
    );
  });

  it('renders helper and warning lines and joins their ids for the control', () => {
    renderUi(
      <Field
        label="Executable"
        helper="Leave blank to detect it"
        warning="Several possible executables"
      >
        <TextInput />
      </Field>,
    );

    const input = screen.getByLabelText('Executable');
    const helper = screen.getByText('Leave blank to detect it');
    const warning = screen.getByText('Several possible executables');

    expect(helper).toHaveClass('text-xs', 'text-text-dimmed');
    expect(warning).toHaveClass('text-xs', 'text-warning');
    expect(warning.querySelector('svg')).toHaveAttribute('width', '14');
    expect(input).toHaveAttribute(
      'aria-describedby',
      `${helper.id} ${warning.id}`,
    );
  });

  it('respects an explicit control id', () => {
    renderUi(
      <Field label="Arguments" id="arguments-field">
        <TextInput />
      </Field>,
    );

    const input = screen.getByLabelText('Arguments');
    expect(input).toHaveAttribute('id', 'arguments-field');
    expect(screen.getByText('Arguments')).toHaveAttribute(
      'for',
      'arguments-field',
    );
  });
});
