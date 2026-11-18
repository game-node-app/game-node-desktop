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
    [
      'en',
      'Sensitive and custom data sharing',
      ['Game paths', 'Game performance history', 'Runtime catalog', 'Content availability', 'Library roots', 'Save profiles'],
      ['Review data sources', 'Review recipients', 'Review shared fields', 'Review historical sharing', 'Review live delivery', 'Review received-data retention'],
    ],
    [
      'pt-BR',
      'Compartilhamento de dados sensíveis e personalizados',
      ['Caminhos dos jogos', 'Histórico de desempenho dos jogos', 'Catálogo de runtimes', 'Disponibilidade de conteúdo', 'Raízes de bibliotecas', 'Perfis de saves'],
      ['Revisar fontes dos dados', 'Revisar destinatários', 'Revisar campos compartilhados', 'Revisar compartilhamento do histórico', 'Revisar entrega em tempo real', 'Revisar retenção dos dados recebidos'],
    ],
  ] as const)('shows all-off grants, private query defaults and separate sensitive review in %s', (locale, sensitiveTitle, categoryLabels, sensitiveLabels) => {
    const onContinue = vi.fn();
    renderPermissions(locale, onContinue);

    for (const label of categoryLabels) {
      expect(screen.getByRole('switch', { name: label })).not.toBeChecked();
    }
    for (const label of sensitiveLabels) {
      expect(screen.getByRole('switch', { name: label })).not.toBeChecked();
    }
    expect(screen.queryByRole('switch', { name: /common data in plugin queries|dados comuns nas consultas do plugin/i })).not.toBeInTheDocument();
    expect(screen.getByRole('group', { name: sensitiveTitle })).toBeInTheDocument();
    expect(screen.getByText(/does not automatically run queries|não executa consultas.*automaticamente/i)).toBeInTheDocument();
    expect(screen.getByText(/must explicitly request common data on each query|precisa solicitar dados comuns explicitamente em cada consulta/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: /Continue|Continuar/ }));
    expect(onContinue).toHaveBeenCalledWith({ commonDataRead: [], sensitiveDataReview: [] });
  });

  it.each([
    ['en', 'Game performance history'],
    ['pt-BR', 'Histórico de desempenho dos jogos'],
  ] as const)('lets a common-data lookup be selected without a transfer wizard in %s', (locale, performanceLabel) => {
    const onContinue = vi.fn();
    renderPermissions(locale, onContinue);

    fireEvent.click(screen.getByRole('switch', { name: performanceLabel }));
    expect(screen.getByRole('switch', { name: performanceLabel })).toBeChecked();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole('switch', { name: /Review recipients|Revisar destinatários/ }));
    fireEvent.click(screen.getByRole('button', { name: /Continue|Continuar/ }));

    expect(onContinue).toHaveBeenCalledWith({
      commonDataRead: ['gamePerformanceHistory'],
      sensitiveDataReview: ['recipient'],
    });
  });
});
