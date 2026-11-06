import React from 'react';
import { useBootstrapState } from '../../../api';
import { uiStore } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';

export interface AboutPanelProps {
  showLicenseNoticesButtonRef?: React.RefObject<HTMLButtonElement | null>;
}

export const AboutPanel: React.FC<AboutPanelProps> = ({
  showLicenseNoticesButtonRef,
}) => {
  const { t } = useI18n();
  const { data: bootstrap } = useBootstrapState();

  const version = bootstrap?.version ?? '0.1.0';

  const handleOpenLicenseModal = () => {
    uiStore.setState((prev) => ({
      ...prev,
      licenseModalOpen: true,
    }));
  };

  return (
    <div className="settings-section">
      <div className="settings-section">
        <h3 className="settings-section-title">{t('About GameNode Desktop') || 'About GameNode Desktop'}</h3>

        <div className="settings-card" style={{ gap: '16px' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '16px' }}>
            <div
              style={{
                width: '40px',
                height: '40px',
                borderRadius: 'var(--radius-sm, 4px)',
                backgroundColor: 'var(--settings-accent, #f15025)',
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                color: '#ffffff',
                fontWeight: 700,
                fontSize: '18px',
              }}
            >
              GN
            </div>
            <div style={{ display: 'flex', flexDirection: 'column' }}>
              <span className="settings-header-title">GameNode Desktop</span>
              <span className="settings-row-description">
                {t('Version {}', [version]) || `Version ${version}`} · GPL-3.0-only
              </span>
            </div>
          </div>

          <div
            style={{
              height: '1px',
              backgroundColor: 'var(--settings-border-subtle, #2a2a2a)',
            }}
          />

          <span className="settings-row-description">
            {t('GameNode artwork: GPL-3.0-only · Roboto: SIL Open Font License 1.1 · Slint: GPLv3') ||
              'GameNode artwork: GPL-3.0-only · Roboto: SIL Open Font License 1.1 · Slint: GPLv3'}
          </span>

          <div>
            <button
              ref={showLicenseNoticesButtonRef}
              type="button"
              className="settings-button settings-button-secondary"
              onClick={handleOpenLicenseModal}
            >
              {t('Show license notices') || 'Show license notices'}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
