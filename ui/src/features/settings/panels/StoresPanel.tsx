import React, { useState } from 'react';
import { commands, useLibraryView, useLocalGames } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';

export const StoresPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const library = useLibraryView();
  const localGames = useLocalGames();

  const [newFolderInput, setNewFolderInput] = useState('');

  const providers = library?.providers ?? [];
  const failedProviders = library?.failedProviders ?? [];
  const isScanning = library?.scanning ?? false;
  const watches = localGames?.watches ?? [];
  const localBusy = localGames?.busy ?? false;
  const discoveryScanning = localGames?.discoveryScanning ?? false;
  const discoveryError = localGames?.discoveryError ?? null;
  const discoveryIncomplete = localGames?.discoveryIncomplete ?? [];
  const discoveryCompleted = localGames?.discoveryCompleted ?? false;

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

  const handleAddWatchFolder = () => {
    const trimmed = newFolderInput.trim();
    if (trimmed) {
      commands.watchLocalFolder(trimmed);
      setNewFolderInput('');
    } else {
      commands.pickLocalWatchFolder();
    }
  };

  const handleRemoveWatch = (id: string) => {
    commands.removeLocalWatch(id);
  };

  const handleRescanFolders = () => {
    commands.rescanLocalFolders();
  };

  const handleResetIgnores = () => {
    commands.resetLocalIgnores();
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

      {/* DRM-free games: watched folders (moved here from Backups to match the pre-migration Lojas page) */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('DRM-free games') || 'DRM-free games'}</h3>
        <span className="settings-section-description">
          {t('Watched folders and local discoveries.') || 'Watched folders and local discoveries.'}
        </span>

        <div className="settings-card">
          <div style={{ display: 'flex', gap: '8px' }}>
            <input
              type="text"
              className="settings-input"
              value={newFolderInput}
              onChange={(e) => setNewFolderInput(e.target.value)}
              placeholder={t('DRM-free::Folder to watch') || 'Folder to watch'}
              aria-label={t('DRM-free::Folder to watch') || 'Folder to watch'}
            />
            <button
              type="button"
              className="settings-button settings-button-secondary"
              disabled={localBusy}
              onClick={handleAddWatchFolder}
            >
              {t('DRM-free::Watch folder') !== 'DRM-free::Watch folder'
                ? t('DRM-free::Watch folder')
                : (locale === 'pt-BR' ? 'Adicionar pasta' : 'Add folder')}
            </button>
          </div>

          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', marginTop: '8px' }}>
            {watches.length === 0 ? (
              <span className="settings-row-description">
                {locale === 'pt-BR' ? 'Nenhuma pasta monitorada.' : 'No watched folders configured.'}
              </span>
            ) : (
              watches.map((watch) => (
                <div
                  key={watch.id}
                  className="settings-row"
                  style={{
                    padding: '8px 12px',
                    backgroundColor: 'var(--settings-bg-raised, #222222)',
                    borderRadius: 'var(--radius-sm, 4px)',
                  }}
                >
                  <span className="settings-row-title" style={{ wordBreak: 'break-all' }}>
                    {watch.path}
                  </span>
                  <button
                    type="button"
                    className="settings-button settings-button-ghost"
                    onClick={() => handleRemoveWatch(watch.id)}
                  >
                    {t('DRM-free::Remove watch') !== 'DRM-free::Remove watch'
                      ? t('DRM-free::Remove watch')
                      : (locale === 'pt-BR' ? 'Remover' : 'Remove')}
                  </button>
                </div>
              ))
            )}
          </div>

          {discoveryScanning && (
            <span className="settings-row-description" style={{ marginTop: '8px' }}>
              {t('DRM-free::Checking watched folders…') !== 'DRM-free::Checking watched folders…'
                ? t('DRM-free::Checking watched folders…')
                : 'Checking watched folders…'}
            </span>
          )}
          {!discoveryScanning && discoveryError && (
            <span className="settings-row-description" role="alert" style={{ marginTop: '8px', color: 'var(--color-danger, #e5484d)' }}>
              {discoveryError}
            </span>
          )}
          {!discoveryScanning && !discoveryError && discoveryIncomplete.length > 0 && (
            <span className="settings-row-description" style={{ marginTop: '8px' }}>
              {t('DRM-free::Some folders could not be checked completely.') !== 'DRM-free::Some folders could not be checked completely.'
                ? t('DRM-free::Some folders could not be checked completely.')
                : 'Some folders could not be checked completely.'}
            </span>
          )}
          {!discoveryScanning && !discoveryError && discoveryCompleted && (
            <span className="settings-row-description" style={{ marginTop: '8px' }}>
              {t('DRM-free::Recognized games in watched folders are added to your library automatically.') !== 'DRM-free::Recognized games in watched folders are added to your library automatically.'
                ? t('DRM-free::Recognized games in watched folders are added to your library automatically.')
                : 'Recognized games in watched folders are added to your library automatically.'}
            </span>
          )}

          <div style={{ display: 'flex', gap: '8px', marginTop: '12px' }}>
            <button
              type="button"
              className="settings-button settings-button-secondary"
              disabled={localBusy || discoveryScanning}
              onClick={handleRescanFolders}
            >
              {t('DRM-free::Rescan watched folders') !== 'DRM-free::Rescan watched folders'
                ? t('DRM-free::Rescan watched folders')
                : (locale === 'pt-BR' ? 'Reescanear pastas' : 'Rescan folders')}
            </button>
            <button
              type="button"
              className="settings-button settings-button-ghost"
              disabled={localBusy}
              onClick={handleResetIgnores}
            >
              {t('DRM-free::Reset ignored discoveries') !== 'DRM-free::Reset ignored discoveries'
                ? t('DRM-free::Reset ignored discoveries')
                : (locale === 'pt-BR' ? 'Redefinir candidatos ignorados' : 'Reset ignored candidates')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
