import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { appTheme } from '../../AppProviders';
import { I18nProvider } from '../../i18n';
import type { SharedDataCatalogRecord } from '../../api/types';
import { PluginActivationPermissions, type PluginActivationPermissionsValue } from './PluginActivationPermissions';

const consumerId = 'org.example.consumer';
const catalog: SharedDataCatalogRecord[] = [
  {
    sourcePluginId: 'org.example.source',
    sourcePublisherFingerprint: 'SHA256:source',
    datasetId: 'org.example.metrics.v1',
    class: 'custom',
    category: null,
    recordId: 'custom-record-1',
    fields: ['fps', 'opaque_payload'],
  },
  {
    sourcePluginId: 'org.example.telemetry',
    sourcePublisherFingerprint: 'SHA256:telemetry',
    datasetId: 'performance',
    class: 'common',
    category: 'gamePerformanceHistory',
    recordId: 'common-record-1',
    fields: ['fps'],
  },
];

function renderPermissions(
  locale: 'en' | 'pt-BR',
  onSave: (value: PluginActivationPermissionsValue) => void,
  initialValue?: PluginActivationPermissionsValue,
) {
  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>
        <PluginActivationPermissions pluginId={consumerId} catalog={catalog} initialValue={initialValue} onSave={onSave} />
      </I18nProvider>
    </MantineProvider>,
  );
}

describe('PluginActivationPermissions', () => {
  it.each([
    ['en', 'Game performance history', 'Save data permissions'],
    ['pt-BR', 'Histórico de desempenho dos jogos', 'Salvar permissões de dados'],
  ] as const)('defaults to private data and explains separation from retention in %s', (locale, performanceLabel, saveLabel) => {
    const onSave = vi.fn();
    renderPermissions(locale, onSave);

    expect(screen.getByRole('switch', { name: performanceLabel })).not.toBeChecked();
    expect(screen.getByRole('checkbox', { name: /common-record-1/ })).not.toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'fps' })).not.toBeChecked();
    expect(screen.getByText(/storage\.persist/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: saveLabel })).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: saveLabel }));
    expect(onSave).toHaveBeenCalledWith({ commonDataRead: [], selectedRecordIds: [], sourcePolicies: [] });
  });

  it('saves category, record, and exact source field/history/live choices together', () => {
    const onSave = vi.fn();
    renderPermissions('en', onSave);

    fireEvent.click(screen.getByRole('switch', { name: 'Game performance history' }));
    fireEvent.click(screen.getByRole('checkbox', { name: /common-record-1/ }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'fps' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'opaque_payload' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Allow historical reads' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Allow live updates' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save data permissions' }));

    expect(onSave).toHaveBeenCalledWith({
      commonDataRead: ['gamePerformanceHistory'],
      selectedRecordIds: ['common-record-1'],
      sourcePolicies: [{
        sourcePluginId: 'org.example.source',
        sourcePublisherFingerprint: 'SHA256:source',
        datasetId: 'org.example.metrics.v1',
        class: 'custom',
        fields: ['fps', 'opaque_payload'],
        history: true,
        live: true,
      }],
    });
  });

  it('restores persisted review choices and lets the user revoke a dataset policy', () => {
    const onSave = vi.fn();
    const initialValue: PluginActivationPermissionsValue = {
      commonDataRead: ['gamePerformanceHistory'],
      selectedRecordIds: ['common-record-1'],
      sourcePolicies: [{
        sourcePluginId: 'org.example.source',
        sourcePublisherFingerprint: 'SHA256:source',
        datasetId: 'org.example.metrics.v1',
        class: 'custom',
        fields: ['fps'],
        history: true,
        live: false,
      }],
    };
    renderPermissions('en', onSave, initialValue);

    expect(screen.getByRole('switch', { name: 'Game performance history' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: /common-record-1/ })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'fps' })).toBeChecked();
    fireEvent.click(screen.getByRole('checkbox', { name: 'fps' }));
    fireEvent.click(screen.getByRole('button', { name: 'Save data permissions' }));

    expect(onSave).toHaveBeenCalledWith({
      commonDataRead: ['gamePerformanceHistory'],
      selectedRecordIds: ['common-record-1'],
      sourcePolicies: [],
    });
  });
});
