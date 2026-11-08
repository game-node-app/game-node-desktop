import React from 'react';
import {
  IconDeviceDesktop,
  IconListCheck,
} from '@tabler/icons-react';
import type { PortEntryView } from '../../../api/ports';
import { useI18n } from '../../../i18n';

type PillTone = 'neutral' | 'brand' | 'success' | 'warning' | 'danger';

const toneClass: Record<PillTone, string> = {
  neutral: 'bg-raised text-text-default',
  brand: 'bg-accent-subtle text-brand',
  success: 'bg-success-subtle text-success',
  warning: 'bg-warning-subtle text-warning',
  danger: 'bg-danger-subtle text-danger',
};

export interface PortPillProps {
  tone?: PillTone;
  children: React.ReactNode;
  testId?: string;
}

export const PortPill: React.FC<PortPillProps> = ({
  tone = 'neutral',
  children,
  testId,
}) => (
  <span
    data-testid={testId}
    className={`inline-flex shrink-0 items-center rounded-full px-1.5 py-px text-[11px] font-medium leading-4 ${toneClass[tone]}`}
  >
    {children}
  </span>
);

export interface PortReleaseBadgeProps {
  entry: PortEntryView;
  /** Detail data can refine the listing (a release withdrawn after the snapshot). */
  withdrawn?: boolean;
  prerelease?: boolean;
  verified?: boolean;
}

/**
 * The release state of an entry, highest severity first: a withdrawn entry never also
 * shows "verified" or "pre-release", and pre-release wins over verified so the tile is
 * honest about the only assets that exist.
 */
export const PortReleaseBadge: React.FC<PortReleaseBadgeProps> = ({
  entry,
  withdrawn,
  prerelease,
  verified,
}) => {
  const { t } = useI18n();
  const isWithdrawn = withdrawn ?? entry.withdrawn;
  const isPrerelease = prerelease ?? entry.prerelease;
  const isVerified = verified ?? entry.verified;
  if (isWithdrawn) {
    return <PortPill tone="danger" testId="port-badge-withdrawn">{t('Ports::Withdrawn')}</PortPill>;
  }
  if (isPrerelease) {
    return (
      <PortPill tone="warning" testId="port-badge-prerelease">
        {t('Ports::Pre-release')}
      </PortPill>
    );
  }
  if (isVerified) {
    return (
      <PortPill tone="success" testId="port-badge-verified">
        {t('Ports::Verified release')}
      </PortPill>
    );
  }
  return null;
};

/** The secondary state icons: guided setup, then installed by GameNode. */
export const PortEntryIcons: React.FC<{ entry: PortEntryView }> = ({ entry }) => {
  const { t } = useI18n();
  return (
    <>
      {entry.guided && (
        <span
          data-testid="port-icon-guided"
          role="img"
          aria-label={t('Ports::Guided setup')}
          title={t('Ports::Guided setup')}
          className="shrink-0 text-text-dimmed"
        >
          <IconListCheck size={12} stroke={1.75} aria-hidden />
        </span>
      )}
      {entry.installedRegistrationKey && (
        <span
          data-testid="port-icon-installed"
          role="img"
          aria-label={t('Ports::Installed')}
          title={t('Ports::Installed')}
          className="shrink-0 text-brand"
        >
          <IconDeviceDesktop size={12} stroke={1.75} aria-hidden />
        </span>
      )}
    </>
  );
};

/** Row variant used by the list view: the release pill plus the state icons. */
export const PortEntryBadges: React.FC<{ entry: PortEntryView }> = ({ entry }) => (
  <div className="flex min-w-0 items-center gap-1 overflow-hidden">
    <PortReleaseBadge entry={entry} />
    <span className="ml-auto flex shrink-0 items-center gap-1">
      <PortEntryIcons entry={entry} />
    </span>
  </div>
);
