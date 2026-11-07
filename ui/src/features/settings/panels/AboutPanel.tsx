import React from 'react';
import { useAppInfo } from '../../../api';
import { uiStore } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import {
  settingsButtonSecondary,
  settingsCardRoomy,
  settingsRowDescription,
  settingsSection,
  settingsSectionTitle,
} from '../settingsClasses';

export interface AboutPanelProps {
  showLicenseNoticesButtonRef?: React.RefObject<HTMLButtonElement | null>;
}

export const AboutPanel: React.FC<AboutPanelProps> = ({
  showLicenseNoticesButtonRef,
}) => {
  const { t } = useI18n();
  const { data: appInfo } = useAppInfo();

  const version = appInfo?.version ?? '0.1.0';

  const handleOpenLicenseModal = () => {
    uiStore.setState((prev) => ({
      ...prev,
      licenseModalOpen: true,
    }));
  };

  return (
    <div className={settingsSection}>
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('About GameNode Desktop') || 'About GameNode Desktop'}</h3>

        <div className={settingsCardRoomy}>
          <div className="flex items-center gap-md">
            <div className="flex h-10 w-10 items-center justify-center rounded-sm bg-brand text-lg font-bold text-white">
              GN
            </div>
            <div className="flex flex-col">
              <span className="text-lg font-bold text-white">GameNode Desktop</span>
              <span className={settingsRowDescription}>
                {t('Version {}', [version]) || `Version ${version}`} · GPL-3.0-only
              </span>
            </div>
          </div>

          <div className="h-px bg-border-subtle" />

          <span className={settingsRowDescription}>
            {t('GameNode artwork: GPL-3.0-only · Roboto: SIL Open Font License 1.1 · Slint: GPLv3') ||
              'GameNode artwork: GPL-3.0-only · Roboto: SIL Open Font License 1.1 · Slint: GPLv3'}
          </span>

          <div>
            <button
              ref={showLicenseNoticesButtonRef}
              type="button"
              className={settingsButtonSecondary}
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
