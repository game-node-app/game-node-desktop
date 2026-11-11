import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { appTheme } from '../../AppProviders';
import { I18nProvider } from '../../i18n';
import { PluginActivationPermissions, type PluginActivationPermissionsValue } from './PluginActivationPermissions';

function renderPermissions(locale: 'en' | 'pt-BR', onContinue: (value: PluginActivationPermissionsValue) => void) {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <PluginActivationPermissions onContinue={onContinue} />
      </I18nProvider>
    </MantineProvider>,
  );
}

describe('PluginActivationPermissions', () => {
  it.each([
    ['en', 'Game library and available ROMs', 'Sensitive and custom data sharing'],
    ['pt-BR', 'Biblioteca de jogos e ROMs disponíveis', 'Compartilhamento de dados sensíveis e personalizados'],
  ] as const)('shows all-off grants, per-query private default and distinct sensitive review in %s', (locale, libraryLabel, sensitiveTitle) => {
    const onContinue = vi.fn();
    renderPermissions(locale, onContinue);

    expect(screen.getByRole('switch', { name: libraryLabel })).not.toBeChecked();
    expect(screen.getByRole('switch', { name: /FPS/ })).not.toBeChecked();
    expect(screen.queryByRole('switch', { name: /common data in plugin queries|dados comuns nas consultas do plugin/i })).not.toBeInTheDocument();
    expect(screen.getByRole('group', { name: sensitiveTitle })).toBeInTheDocument();
    expect(screen.getByText(/does not automatically run queries|não executa consultas.*automaticamente/i)).toBeInTheDocument();
    expect(screen.getByText(/must explicitly request common data on each query|precisa solicitar dados comuns explicitamente em cada consulta/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Continue|Continuar/ }));
    expect(onContinue).toHaveBeenCalledWith({ commonDataRead: [], sensitiveDataReview: [] });
  });

  it('returns common grants separately from sensitive review', () => {
    const onContinue = vi.fn();
    renderPermissions('en', onContinue);

    fireEvent.click(screen.getByRole('switch', { name: 'Performance history (FPS and related metrics)' }));
    fireEvent.click(screen.getByRole('switch', { name: 'Review recipients' }));
    fireEvent.click(screen.getByRole('button', { name: 'Continue' }));

    expect(onContinue).toHaveBeenCalledWith({
      commonDataRead: ['performance'],
      sensitiveDataReview: ['recipient'],
    });
  });
});
