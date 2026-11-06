import React from 'react';
import { FloatingModal } from '../../components/FloatingModal';
import { useLicenseNotices } from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonSecondary } from './settingsClasses';

export interface LicenseNoticesModalProps {
  onClose: () => void;
}

export const LicenseNoticesModal: React.FC<LicenseNoticesModalProps> = ({ onClose }) => {
  const { t } = useI18n();
  const { data: notices } = useLicenseNotices();
  const text = notices ?? 'GameNode Desktop - GPL-3.0-only';

  return (
    <FloatingModal.Root
      opened
      onClose={onClose}
      geometryKey="license-notices"
      defaultSize={{ width: 720, height: 560 }}
      minSize={{ width: 480, height: 360 }}
      zIndex={1010}
      closeOnClickOutside
    >
      <FloatingModal.Overlay backgroundOpacity={0.65} />
      <FloatingModal.Content
        aria-label={t('License notices')}
        className="flex flex-col bg-secondary text-white"
      >
        <FloatingModal.Header>
          <FloatingModal.Title>{t('License notices')}</FloatingModal.Title>
          <FloatingModal.CloseButton data-autofocus onClick={onClose} />
        </FloatingModal.Header>

        <FloatingModal.Body
          tabIndex={0}
          className="whitespace-pre-wrap px-control-inline py-lg font-mono text-xs leading-normal text-[var(--settings-text-secondary)] focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none"
        >
          {text}
        </FloatingModal.Body>

        <FloatingModal.Footer className="bg-[var(--settings-bg-base)]">
          <button type="button" className={settingsButtonSecondary} onClick={onClose}>
            {t('Close') || 'Close'}
          </button>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
