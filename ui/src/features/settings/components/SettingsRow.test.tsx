import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { IconAlertTriangle } from '@tabler/icons-react';
import { Switch } from '../../../components/ui/Switch';
import { renderUiWithI18n } from '../../../components/ui/testUtils';
import { SettingsRow } from './SettingsRow';
import { SettingsSearchProvider } from './SettingsSearchContext';

describe('SettingsRow', () => {
  it('labels and describes its control through the render prop ids', () => {
    renderUiWithI18n(
      <SettingsRow
        label="Sync with GameNode"
        help="Store playtime is sent to your account."
        control={(ids) => (
          <Switch
            aria-labelledby={ids.labelId}
            aria-describedby={ids.describedBy}
          />
        )}
      />,
    );

    const control = screen.getByRole('switch');
    expect(control).toHaveAccessibleName('Sync with GameNode');
    expect(control).toHaveAccessibleDescription(
      'Store playtime is sent to your account.',
    );
    expect(screen.getByText('Sync with GameNode')).toHaveClass(
      'text-sm',
      'font-medium',
      'text-text-primary',
    );
    expect(screen.getByText('Store playtime is sent to your account.')).toHaveClass(
      'max-w-settings-copy',
      'text-xs',
      'leading-note',
      'text-text-dimmed',
    );
  });

  it('renders a plain control node and the row border', () => {
    renderUiWithI18n(
      <SettingsRow
        label="Start with Windows"
        control={<button type="button">Toggle</button>}
      />,
    );

    expect(
      screen.getByRole('button', { name: 'Toggle' }),
    ).toBeInTheDocument();
    expect(screen.getByText('Start with Windows')).toBeInTheDocument();
  });

  it('marks changed rows with the brand dot, previous value and description', () => {
    renderUiWithI18n(
      <SettingsRow
        label="Backup destination"
        help="Local folder where backups are saved."
        changed
        previous={'D:\\Saved'}
        control={(ids) => (
          <input
            aria-label="Backup destination"
            aria-describedby={ids.describedBy}
            readOnly
            value={'E:\\New'}
          />
        )}
      />,
    );

    const dot = screen.getByTestId('settings-row-changed-dot');
    expect(dot).toHaveAttribute('aria-hidden', 'true');
    expect(dot).toHaveClass(
      'absolute',
      'left-[6px]',
      'h-[6px]',
      'w-[6px]',
      'rounded-full',
      'bg-brand',
    );
    expect(screen.getByText('Before: D:\\Saved')).toBeInTheDocument();
    expect(
      screen.queryByText('Local folder where backups are saved.'),
    ).not.toBeInTheDocument();
    expect(screen.getByRole('textbox', { name: 'Backup destination' }))
      .toHaveAccessibleDescription('Before: D:\\Saved Changed');
  });

  it('renders a compact row with a warning icon and a 13px label', () => {
    renderUiWithI18n(
      <SettingsRow
        data-testid="settings-row"
        label={'F:\\Portable'}
        compact
        labelSize="compact"
        icon={IconAlertTriangle}
        iconTone="warning"
      />,
    );

    const row = screen.getByTestId('settings-row');
    expect(row).toHaveClass('gap-row', 'py-sm', 'flex-wrap');
    expect(screen.getByText('F:\\Portable')).toHaveClass('text-body');
    expect(row.querySelector('svg')).toHaveClass('text-warning');
  });

  it('links the label to a control with htmlFor', () => {
    renderUiWithI18n(
      <SettingsRow
        label="Backup destination"
        htmlFor="backup-destination"
        control={
          <input id="backup-destination" aria-label="Backup destination" readOnly />
        }
      />,
    );

    const input = screen.getByLabelText('Backup destination');
    expect(input).toHaveAttribute('id', 'backup-destination');
    const label = screen.getByText('Backup destination');
    expect(label.tagName).toBe('LABEL');
    expect(label).toHaveAttribute('for', 'backup-destination');
  });

  it('marks a row whose label matches the settings search', () => {
    renderUiWithI18n(
      <SettingsSearchProvider query="bandeja">
        <SettingsRow label="Fechar a janela minimiza para a bandeja" />
        <SettingsRow label="Visualização padrão" />
      </SettingsSearchProvider>,
    );

    const match = screen.getByText('Fechar a janela minimiza para a bandeja');
    expect(match).toHaveClass('decoration-brand');
    expect(match.closest('[data-search-match="true"]')).not.toBeNull();
    expect(
      screen
        .getByText('Visualização padrão')
        .closest('[data-search-match="true"]'),
    ).toBeNull();
  });
});
