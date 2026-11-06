import React from 'react';
import { commands, useAuth } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';

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
    <div className="settings-section">
      {/* Session and Identity */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('settings.sessionAndIdentity') || 'Session and identity'}</h3>
        <div className="settings-card">
          {isSignedIn ? (
            <div className="settings-row">
              <div className="settings-row-info">
                <span className="settings-row-title">{identity}</span>
                <span className="settings-row-description">{getStorageText()}</span>
              </div>
              <button
                type="button"
                className="settings-button settings-button-danger"
                onClick={handleSignOut}
              >
                {t('Sign out') || 'Sign out'}
              </button>
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              <span className="settings-row-title">{t('settings.noSession') || 'No session'}</span>
              <span className="settings-row-description">
                {t('Sign in with your GameNode account in your browser.')}
              </span>
            </div>
          )}
        </div>
      </div>

      {/* Sync with GameNode */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Sync') || 'Sync'}</h3>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-info">
              <span className="settings-row-title">{t('Sync with GameNode')}</span>
              <span className="settings-row-description">
                {t('Playtime totals are sent to your GameNode account. You can turn sync off in Settings and keep using your library.')}
              </span>
            </div>
            <label className="settings-toggle">
              <input
                type="checkbox"
                checked={sync.enabled}
                onChange={handleToggleSync}
                aria-label={t('Sync with GameNode')}
              />
              <span className="settings-toggle-slider" />
            </label>
          </div>
        </div>

        {playtimeError && (
          <div className="settings-info-box warning" role="alert">
            <span>⚠</span>
            <span>{t('Could not sync playtime. Your local data is preserved.')}</span>
          </div>
        )}
      </div>

      {/* Playtime Source */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Playtime source')}</h3>
        <span className="settings-section-description">
          {t('AppWindow::Applies to all games on this PC. Games without a supported store counter always use the internal counter. Store and internal histories stay separate.')}
        </span>
        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-info">
              <span className="settings-row-title">{t('Playtime source')}</span>
            </div>
            <div className="settings-segmented" role="radiogroup" aria-label={t('Playtime source')}>
              <button
                type="button"
                role="radio"
                aria-checked={playtimeSource === 'store'}
                className={`settings-segmented-option ${playtimeSource === 'store' ? 'selected' : ''}`}
                onClick={() => handleSelectPlaytimeSource('store')}
              >
                {t('Store') || 'Store'}
              </button>
              <button
                type="button"
                role="radio"
                aria-checked={playtimeSource === 'internal'}
                className={`settings-segmented-option ${playtimeSource === 'internal' ? 'selected' : ''}`}
                onClick={() => handleSelectPlaytimeSource('internal')}
              >
                {t('Internal') || 'Internal'}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};
