import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { Field } from './Field';
import { TextArea } from './TextArea';
import { renderUi } from './testUtils';

describe('TextArea', () => {
  it('renders the 64px multiline surface with the 19px line height', () => {
    renderUi(<TextArea aria-label="Arguments" defaultValue="--windowed" />);

    const textarea = screen.getByRole('textbox', { name: 'Arguments' });
    expect(textarea).toHaveClass(
      'h-[64px]',
      'resize-none',
      'rounded-control',
      'border',
      'border-border-subtle',
      'bg-surface',
      'px-xs',
      'pt-compact',
      'text-body',
      'leading-field',
      'text-text-primary',
      'focus:border-brand',
      'placeholder:text-text-dimmed',
    );
  });

  it('binds to the Field label and the helper text', () => {
    renderUi(
      <Field
        label="Game executables (one per line)"
        helper="Names only, no paths"
      >
        <TextArea />
      </Field>,
    );

    const textarea = screen.getByLabelText('Game executables (one per line)');
    const helper = screen.getByText('Names only, no paths');
    expect(textarea).toHaveAttribute('aria-describedby', helper.id);
  });

  it('keeps placeholder and value content accessible', () => {
    renderUi(
      <TextArea aria-label="Launchers" placeholder="launcher.exe" />,
    );

    const textarea = screen.getByPlaceholderText('launcher.exe');
    expect(textarea).toHaveAttribute('placeholder', 'launcher.exe');
    expect(textarea).toHaveClass('placeholder:text-text-dimmed');
  });
});
