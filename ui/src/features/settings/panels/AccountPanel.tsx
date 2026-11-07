import React from 'react';
import { SegmentedControl, Switch } from '@mantine/core';
import { IconAlertTriangle } from '@tabler/icons-react';
import { commands, useAuth } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import {
  settingsButtonDanger,
  settingsCard,
  settingsInfoBoxWarning,
  settingsRow,
  settingsRowDescription,
  settingsRowInfo,
  settingsRowTitle,
  settingsSection,
  settingsSectionDescription,
  settingsSectionTitle,
} from '../settingsClasses';

export const AccountPanel: React.FC = () => {
  const { t } = useI18n();
  const { auth, storage, sync, playtimeSource, playtimeError } = useAuth();

  const isSignedIn = typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const user = isSignedIn ? auth.signedIn.user : null;
  const identity = user?.username ?? user?.email ?? user?.id ?? '';

  const getStorageText = () => {
    switch (storage) {
      case 'windowsCredentialManager':
        return t('app.sessionStorageWindowsCredentialManager');
      case 'secretService':
        return t('app.sessionStorageSecretService');
      case 'privateFile':
        return t('app.sessionStorageAFileReadableOnlyBy');
      default:
        return storage;
    }
  };

  const handleToggleSync = () => {
    const next = !sync.enabled;
    commands.setSyncEnabled(next);
    pushToast('success', t('Sync with GameNode'));
  };

  const handleSelectPlaytimeSource = (source: 'store' | 'internal') => {
    commands.setPlaytimeSource(source);
    pushToast('success', t('Playtime source'));
  };

  const handleSignOut = () => {
    commands.dispatchAuth('signOut');
  };

  return (
    <div className={settingsSection}>
      {/* Session and Identity */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('settings.sessionAndIdentity') || 'Session and identity'}</h3>
        <div className={settingsCard}>
          {isSignedIn ? (
            <div className={settingsRow}>
              <div className={settingsRowInfo}>
                <span className={settingsRowTitle}>{identity}</span>
                <span className={settingsRowDescription}>{getStorageText()}</span>
              </div>
              <button
                type="button"
                className={settingsButtonDanger}
                onClick={handleSignOut}
              >
                {t('Sign out') || 'Sign out'}
              </button>
            </div>
          ) : (
            <div className="flex flex-col gap-compact">
              <span className={settingsRowTitle}>{t('settings.noSession') || 'No session'}</span>
              <span className={settingsRowDescription}>
                {t('Sign in with your GameNode account in your browser.')}
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Sync with GameNode */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Sync') || 'Sync'}</h3>
        <div className={settingsCard}>
          <div className={settingsRow}>
            <div className={settingsRowInfo}>
              <span className={settingsRowTitle}>{t('Sync with GameNode')}</span>
              <span className={settingsRowDescription}>
                {t('Playtime totals and favorite changes are sent to your GameNode account. You can turn sync off in Settings and keep using your library.')}
              </span>
            </div>
            <Switch
              aria-label={t('Sync with GameNode')}
              checked={sync.enabled}
              onChange={handleToggleSync}
              color="brand"
              size="md"
            />
          </div>
        </div>

        {playtimeError && (
          <div className={settingsInfoBoxWarning} role="alert">
            <IconAlertTriangle size={14} stroke={1.75} aria-hidden="true" />
            <span>{t('Could not sync playtime. Your local data is preserved.')}</span>
          </div>
        )}
      </div>

      {/* Playtime Source */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Playtime source')}</h3>
        <span className={settingsSectionDescription}>
          {t('AppWindow::Applies to all games on this PC. Games without a supported store counter always use the internal counter. Store and internal histories stay separate.')}
        </span>
        <div className={settingsCard}>
          <div className={settingsRow}>
            <div className={settingsRowInfo}>
              <span className={settingsRowTitle}>{t('Playtime source')}</span>
            </div>
            <SegmentedControl
              aria-label={t('Playtime source')}
              value={playtimeSource}
              onChange={(value) => handleSelectPlaytimeSource(value as 'store' | 'internal')}
              data={[
                { value: 'store', label: t('Store') || 'Store' },
                { value: 'internal', label: t('Internal') || 'Internal' },
              ]}
              color="brand"
              size="xs"
              radius="sm"
            />
          </div>
        </div>
      </div>
    </div>
  );
};
