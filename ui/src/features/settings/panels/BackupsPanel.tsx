import React from 'react';
import { commands, useLocalGames } from '../../../api';
import { pushToast, uiStore, useUiStore } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';

export const BackupsPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const localGames = useLocalGames();
  const backupDestinationDraft = useUiStore((s) => s.backupDestinationDraft);

  const currentDestination = localGames?.settings?.destination ?? '';
  const isAutomatic = localGames?.settings?.automatic ?? false;
  const draftValue = backupDestinationDraft !== null ? backupDestinationDraft : currentDestination;
  const isDirty = backupDestinationDraft !== null && backupDestinationDraft !== currentDestination;

  const handleToggleAutomatic = () => {
    commands.setLocalBackupSettings(!isAutomatic, draftValue);
  };

  const handleChangeDestination = (value: string) => {
    uiStore.setState((prev) => ({
      ...prev,
      backupDestinationDraft: value,
    }));
  };

  const handleBrowseDestination = async () => {
    const picked = await commands.pickSaveDestination();
    if (picked) {
      uiStore.setState((prev) => ({
        ...prev,
        backupDestinationDraft: picked,
      }));
    }
  };

  const handleSaveDestination = () => {
    commands.setLocalBackupSettings(isAutomatic, draftValue);
    uiStore.setState((prev) => ({
      ...prev,
      backupDestinationDraft: null,
    }));
    pushToast('success', t('Backup destination saved'));
  };

  const handleRevertDestination = () => {
    uiStore.setState((prev) => ({
      ...prev,
      backupDestinationDraft: null,
    }));
  };

  return (
    <div className="settings-section">
      <div className="settings-info-box">
        <span>🛡</span>
        <span>
          {t('DRM-free::Backups stay on this PC. A backup on the same drive cannot protect against drive loss.') ||
            'Backups stay on this PC. A backup on the same drive cannot protect against drive loss.'}
        </span>
      </div>

      {/* Automatic Protection */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Automatic protection') || 'Automatic protection'}</h3>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-info">
              <span className="settings-row-title">{t('Automatic backups') || 'Automatic backups'}</span>
              <span className="settings-row-description">
                {t('Generates local backups automatically.') || 'Generates local backups automatically.'}
              </span>
            </div>
            <label className="settings-toggle">
              <input
                type="checkbox"
                checked={isAutomatic}
                onChange={handleToggleAutomatic}
                aria-label={t('Automatic backups') || 'Automatic backups'}
              />
              <span className="settings-toggle-slider" />
            </label>
          </div>
        </div>
      </div>

      {/* Backup Destination */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Backup destination') || 'Backup destination'}</h3>
        <span className="settings-section-description">
          {t('Local folder where DRM-free game backups are saved.') ||
            'Local folder where DRM-free game backups are saved.'}
        </span>
        <div className="settings-card">
          <div style={{ display: 'flex', gap: '8px' }}>
            <input
              type="text"
              className="settings-input"
              value={draftValue}
              onChange={(e) => handleChangeDestination(e.target.value)}
              aria-label={t('Backup destination') || 'Backup destination'}
            />
            <button
              type="button"
              className="settings-button settings-button-secondary"
              onClick={handleBrowseDestination}
            >
              {t('Choose folder…') || 'Browse'}
            </button>
          </div>

          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginTop: '8px',
            }}
          >
            {isDirty ? (
              <span className="settings-pill settings-pill-warning">
                {t('Unsaved changes') || 'Unsaved changes'}
              </span>
            ) : <span />}

            <div style={{ display: 'flex', gap: '8px' }}>
              <button
                type="button"
                className="settings-button settings-button-ghost"
                disabled={!isDirty}
                onClick={handleRevertDestination}
              >
                {t('Revert') || (locale === 'pt-BR' ? 'Reverter' : 'Revert')}
              </button>
              <button
                type="button"
                className="settings-button settings-button-primary"
                disabled={!isDirty}
                onClick={handleSaveDestination}
              >
                {t('Save changes') || 'Save'}
              </button>
            </div>
          </div>
        </div>
      </div>

    </div>
  );
};
