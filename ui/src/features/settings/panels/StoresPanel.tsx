import React from 'react';
import { commands, useLibraryView } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';

export const StoresPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const library = useLibraryView();

  const providers = library?.providers ?? [];
  const failedProviders = library?.failedProviders ?? [];
  const isScanning = library?.scanning ?? false;

  const handleRescan = () => {
    commands.rescanLibrary();
  };

  const handleResetExcluded = () => {
    commands.libraryInput('resetExcluded');
    const toastMsg =
      locale === 'pt-BR'
        ? 'Descobertas ignoradas redefinidas'
        : 'Reset hidden games';
    pushToast('success', t('Reset hidden games') !== 'Reset hidden games' ? t('Reset hidden games') : toastMsg);
  };

  return (
    <div className="settings-section">
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Detected stores') || 'Detected stores'}</h3>
        <span className="settings-section-description">
          {t('Game providers configured on this system.')}
        </span>

        <div className="settings-card">
          {providers.length === 0 ? (
            <div className="settings-row-description">
              {t('No installed games found') || 'No installed games found'}
            </div>
          ) : (
            providers.map((provider) => {
              const hasFailed = failedProviders.includes(provider.id);
              let description = '';
              if (hasFailed) {
                description = t('Last scan failed') || 'Last scan failed';
              } else if (provider.count > 0) {
                description = `${provider.count} ${t('installed games') || 'installed games'}`;
              } else {
                description = t('No installed games found') || 'No installed games found';
              }

              return (
                <div key={provider.id} className="settings-row" style={{ padding: '4px 0' }}>
                  <div className="settings-row-info">
                    <span className="settings-row-title">{provider.name}</span>
                    <span className="settings-row-description">{description}</span>
                  </div>
                  {hasFailed && (
                    <span className="settings-pill settings-pill-warning">
                      {t('Failed') || 'Failed'}
                    </span>
                  )}
                </div>
              );
            })
          )}
        </div>

        <div style={{ display: 'flex', gap: '8px', alignItems: 'center', marginTop: '8px' }}>
          <button
            type="button"
            className="settings-button settings-button-secondary"
            disabled={isScanning}
            onClick={handleRescan}
          >
            {t('Library::Rescan') !== 'Library::Rescan' ? t('Library::Rescan') : (t('Rescan') || 'Rescan library')}
          </button>
          <button
            type="button"
            className="settings-button settings-button-ghost"
            onClick={handleResetExcluded}
          >
            {t('Reset hidden games') !== 'Reset hidden games' ? t('Reset hidden games') : (locale === 'pt-BR' ? 'Restaurar entradas excluídas' : 'Reset hidden games')}
          </button>
        </div>
      </div>
    </div>
  );
};
