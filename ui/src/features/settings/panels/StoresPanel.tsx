import React, { useState } from 'react';
import { commands, useLibraryView, useLocalGames } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import {
  settingsButtonGhost,
  settingsButtonSecondary,
  settingsCard,
  settingsInput,
  settingsPillWarning,
  settingsRow,
  settingsRowDescription,
  settingsRowInfo,
  settingsRowTitle,
  settingsSection,
  settingsSectionDescription,
  settingsSectionTitle,
} from '../settingsClasses';

export const StoresPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const library = useLibraryView();
  const localGames = useLocalGames();

  const [newFolderInput, setNewFolderInput] = useState('');

  const providers = library?.providers ?? [];
  const failedProviders = library?.failedProviders ?? [];
  const isScanning = library?.scanning ?? false;
  // Removing a watch disables it but keeps its registrations (games stay in the library).
  // Disabled roots must not be listed as watched.
  const watches = (localGames?.watches ?? []).filter((watch) => watch.enabled);
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
    <div className={settingsSection}>
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Detected stores') || 'Detected stores'}</h3>
        <span className={settingsSectionDescription}>
          {t('Game providers configured on this system.')}
        </span>

        <div className={settingsCard}>
          {providers.length === 0 ? (
            <div className={settingsRowDescription}>
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
                <div key={provider.id} className={`${settingsRow} py-1`}>
                  <div className={settingsRowInfo}>
                    <span className={settingsRowTitle}>{provider.name}</span>
                    <span className={settingsRowDescription}>{description}</span>
                  </div>
                  {hasFailed && (
                    <span className={settingsPillWarning}>
                      {t('Failed') || 'Failed'}
                    </span>
                  )}
                </div>
              );
            })
          )}
        </div>

        <div className="mt-compact flex items-center gap-compact">
          <button
            type="button"
            className={settingsButtonSecondary}
            disabled={isScanning}
            onClick={handleRescan}
          >
            {t('Library::Rescan') !== 'Library::Rescan' ? t('Library::Rescan') : (t('Rescan') || 'Rescan library')}
          </button>
          <button
            type="button"
            className={settingsButtonGhost}
            onClick={handleResetExcluded}
          >
            {t('Reset hidden games') !== 'Reset hidden games' ? t('Reset hidden games') : (locale === 'pt-BR' ? 'Restaurar entradas excluídas' : 'Reset hidden games')}
          </button>
        </div>
      </div>

      {/* DRM-free games: watched folders (moved here from Backups to match the pre-migration Lojas page) */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('DRM-free games') || 'DRM-free games'}</h3>
        <span className={settingsSectionDescription}>
          {t('Watched folders and local discoveries.') || 'Watched folders and local discoveries.'}
        </span>

        <div className={settingsCard}>
          <div className="flex gap-compact">
            <input
              type="text"
              className={settingsInput}
              value={newFolderInput}
              onChange={(e) => setNewFolderInput(e.target.value)}
              placeholder={t('DRM-free::Folder to watch') || 'Folder to watch'}
              aria-label={t('DRM-free::Folder to watch') || 'Folder to watch'}
            />
            <button
              type="button"
              className={settingsButtonSecondary}
              disabled={localBusy}
              onClick={handleAddWatchFolder}
            >
              {t('DRM-free::Watch folder') !== 'DRM-free::Watch folder'
                ? t('DRM-free::Watch folder')
                : (locale === 'pt-BR' ? 'Adicionar pasta' : 'Add folder')}
            </button>
          </div>

          <div className="mt-compact flex flex-col gap-compact">
            {watches.length === 0 ? (
              <span className={settingsRowDescription}>
                {locale === 'pt-BR' ? 'Nenhuma pasta monitorada.' : 'No watched folders configured.'}
              </span>
            ) : (
              watches.map((watch) => (
                <div
                  key={watch.id}
                  className={`${settingsRow} rounded-sm bg-raised px-sm py-compact`}
                >
                  <span className={`${settingsRowTitle} break-all`}>
                    {watch.path}
                  </span>
                  <button
                    type="button"
                    className={settingsButtonGhost}
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
            <span className={`${settingsRowDescription} mt-compact`}>
              {t('DRM-free::Checking watched folders…') !== 'DRM-free::Checking watched folders…'
                ? t('DRM-free::Checking watched folders…')
                : 'Checking watched folders…'}
            </span>
          )}
          {!discoveryScanning && discoveryError && (
            <span className="mt-compact text-xs text-danger" role="alert">
              {discoveryError}
            </span>
          )}
          {!discoveryScanning && !discoveryError && discoveryIncomplete.length > 0 && (
            <span className={`${settingsRowDescription} mt-compact`}>
              {t('DRM-free::Some folders could not be checked completely.') !== 'DRM-free::Some folders could not be checked completely.'
                ? t('DRM-free::Some folders could not be checked completely.')
                : 'Some folders could not be checked completely.'}
            </span>
          )}
          {!discoveryScanning && !discoveryError && discoveryCompleted && (
            <span className={`${settingsRowDescription} mt-compact`}>
              {t('DRM-free::Recognized games in watched folders are added to your library automatically.') !== 'DRM-free::Recognized games in watched folders are added to your library automatically.'
                ? t('DRM-free::Recognized games in watched folders are added to your library automatically.')
                : 'Recognized games in watched folders are added to your library automatically.'}
            </span>
          )}

          <div className="mt-sm flex gap-compact">
            <button
              type="button"
              className={settingsButtonSecondary}
              disabled={localBusy || discoveryScanning}
              onClick={handleRescanFolders}
            >
              {t('DRM-free::Rescan watched folders') !== 'DRM-free::Rescan watched folders'
                ? t('DRM-free::Rescan watched folders')
                : (locale === 'pt-BR' ? 'Reescanear pastas' : 'Rescan folders')}
            </button>
            <button
              type="button"
              className={settingsButtonGhost}
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
