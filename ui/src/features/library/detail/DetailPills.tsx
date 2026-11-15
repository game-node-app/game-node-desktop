import {
  IconDeviceDesktop,
  IconDownload,
  IconEyeOff,
  IconFolder,
  IconLink,
  IconShieldCheck,
  IconUsersGroup,
} from '@tabler/icons-react';
import type { DetailView, EntryStatus } from '../../../api';
import { Pill } from '../../../components/ui';
import { useI18n } from '../../../i18n';
import { detailStatusLabel, detailStatusTone } from './format';

export interface DetailPillsProps {
  detail: DetailView;
  isDrmFree: boolean;
  protectionActive: boolean;
  inLibrary: boolean;
  /** `null` means in the library with a status this build does not know. */
  inLibraryStatus: EntryStatus | null;
  isFamilyShared: boolean;
}

/** The state pills under the detail actions. */
export function DetailPills({
  detail,
  isDrmFree,
  protectionActive,
  inLibrary,
  inLibraryStatus,
  isFamilyShared,
}: DetailPillsProps) {
  const { t } = useI18n();
  const row = detail.row;

  return (
    <div data-testid="detail-pills" className="flex flex-wrap items-center gap-[6px]">
      {row.installed ? (
        <span data-testid="detail-pill-installed">
          <Pill tone="success" icon={IconDeviceDesktop}>
            {t('Detail::Installed on this PC')}
          </Pill>
        </span>
      ) : (
        <span data-testid="detail-pill-not-installed">
          <Pill tone="neutral" icon={IconDownload}>
            {t('Detail::Not installed')}
          </Pill>
        </span>
      )}

      {inLibrary &&
        (inLibraryStatus != null ? (
          <span data-testid="detail-pill-status">
            <Pill tone={detailStatusTone(inLibraryStatus)}>
              {detailStatusLabel(inLibraryStatus, t)}
            </Pill>
          </span>
        ) : (
          <span data-testid="detail-pill-status">
            <Pill tone="neutral">{t('Detail::In your GameNode library')}</Pill>
          </span>
        ))}

      {isDrmFree && (
        <span data-testid="detail-pill-drmfree">
          <Pill tone="neutral" icon={IconFolder}>
            {t('Detail::DRM-free')}
          </Pill>
        </span>
      )}

      {isFamilyShared && (
        <span data-testid="detail-pill-family">
          <Pill tone="neutral" icon={IconUsersGroup}>
            {t('Library::Family shared')}
          </Pill>
        </span>
      )}

      {protectionActive && (
        <span data-testid="detail-pill-protection">
          <Pill tone="success" icon={IconShieldCheck}>
            {t('Detail::Saves protected')}
          </Pill>
        </span>
      )}

      {isMatched(detail) && (
        <span data-testid="detail-pill-match">
          <Pill tone="neutral" icon={IconLink}>
            {t('Detail::Matched')}
          </Pill>
        </span>
      )}

      {row.hidden && (
        <span data-testid="detail-pill-hidden">
          <Pill tone="neutral" icon={IconEyeOff}>
            {t('Library::Hidden')}
          </Pill>
        </span>
      )}
    </div>
  );
}

function isMatched(detail: DetailView): boolean {
  return typeof detail.row.status === 'object' && 'matched' in detail.row.status;
}
