import React from 'react';
import { Switch } from '@mantine/core';
import { IconShieldCheck } from '@tabler/icons-react';
import { commands, useLocalGames } from '../../../api';
import { pushToast, uiStore, useUiStore } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import {
  settingsButtonGhost,
  settingsButtonPrimary,
  settingsButtonSecondary,
  settingsCard,
  settingsInfoBox,
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
    <div className={settingsSection}>
      <div className={settingsInfoBox}>
        <IconShieldCheck size={14} stroke={1.75} aria-hidden="true" />
        <span>
          {t('DRM-free::Backups stay on this PC. A backup on the same drive cannot protect against drive loss.') ||
            'Backups stay on this PC. A backup on the same drive cannot protect against drive loss.'}
        </span>
      </div>

      {/* Automatic Protection */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Automatic protection') || 'Automatic protection'}</h3>
        <div className={settingsCard}>
          <div className={settingsRow}>
            <div className={settingsRowInfo}>
              <span className={settingsRowTitle}>{t('Automatic backups') || 'Automatic backups'}</span>
              <span className={settingsRowDescription}>
                {t('Generates local backups automatically.') || 'Generates local backups automatically.'}
              </span>
            </div>
            <Switch
              aria-label={t('Automatic backups') || 'Automatic backups'}
              checked={isAutomatic}
              onChange={handleToggleAutomatic}
              color="brand"
              size="md"
            />
          </div>
        </div>
      </div>

      {/* Backup Destination */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Backup destination') || 'Backup destination'}</h3>
        <span className={settingsSectionDescription}>
          {t('Local folder where DRM-free game backups are saved.') ||
            'Local folder where DRM-free game backups are saved.'}
        </span>
        <div className={settingsCard}>
          <div className="flex gap-compact">
            <input
              type="text"
              className={settingsInput}
              value={draftValue}
              onChange={(e) => handleChangeDestination(e.target.value)}
              aria-label={t('Backup destination') || 'Backup destination'}
            />
            <button
              type="button"
              className={settingsButtonSecondary}
              onClick={handleBrowseDestination}
            >
              {t('Choose folder…') || 'Browse'}
            </button>
          </div>

          <div className="mt-compact flex items-center justify-between">
            {isDirty ? (
              <span className={settingsPillWarning}>
                {t('Unsaved changes') || 'Unsaved changes'}
              </span>
            ) : <span />}

            <div className="flex gap-compact">
              <button
                type="button"
                className={settingsButtonGhost}
                disabled={!isDirty}
                onClick={handleRevertDestination}
              >
                {t('Revert') || (locale === 'pt-BR' ? 'Reverter' : 'Revert')}
              </button>
              <button
                type="button"
                className={settingsButtonPrimary}
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
