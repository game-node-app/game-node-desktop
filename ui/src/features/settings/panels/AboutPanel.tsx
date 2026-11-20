import React, { useState } from 'react';
import {
  IconCopy,
  IconCpu,
  IconExternalLink,
  IconFileText,
  IconFolderOpen,
  IconTerminal2,
} from '@tabler/icons-react';
import {
  commands,
  useAppInfo,
  type GameNodeLink,
} from '../../../api';
import {
  pushToast,
  uiStore,
} from '../../../store/uiStore';
import { copyText } from '../../../diagnostics/crashCopy';
import { useI18n } from '../../../i18n';
import { Button } from '../../../components/ui';
import { RecentLogModal } from '../RecentLogModal';
import { SettingsCard, SettingsPanel, SettingsRow } from '../components';
import wordmark from '../../../assets/icons/gamenode-logo.svg';

export interface AboutPanelProps {
  showLicenseNoticesButtonRef?: React.RefObject<HTMLButtonElement | null>;
}

const ABOUT_LINKS: Array<{ kind: GameNodeLink; labelKey: string }> = [
  { kind: 'website', labelKey: 'settings.about.links.website' },
  { kind: 'terms', labelKey: 'settings.about.links.terms' },
  { kind: 'privacy', labelKey: 'settings.about.links.privacy' },
  { kind: 'reportIssue', labelKey: 'settings.about.links.report' },
];

/**
 * Sobre (S7): the branded version and build card, diagnostics (recent log, log
 * folder and redacted system information copied by the host), the offline
 * license notices and the fixed GameNode website links. There is no app
 * updater here by design (D15).
 */
export const AboutPanel: React.FC<AboutPanelProps> = ({
  showLicenseNoticesButtonRef,
}) => {
  const { t } = useI18n();
  const { data: appInfo } = useAppInfo();
  const [logOpen, setLogOpen] = useState(false);
  const [copying, setCopying] = useState(false);

  const version = appInfo?.version ?? '0.1.0';
  const logDirectory = appInfo?.logDirectory ?? null;
  const buildLine = t('settings.about.buildLine', {
    date: appInfo?.buildDate ?? '—',
    os: appInfo?.osVersion ?? '—',
    arch: appInfo?.arch ?? '—',
  });

  const handleOpenLicenseModal = () => {
    uiStore.setState((prev) => ({
      ...prev,
      licenseModalOpen: true,
    }));
  };

  const handleCopySystemInfo = async () => {
    if (copying) return;
    setCopying(true);
    try {
      const text = await commands.systemInfoText();
      const copied = await copyText(text);
      pushToast(
        copied ? 'success' : 'danger',
        copied
          ? t('settings.about.infoCopied')
          : t('Could not copy to the clipboard.'),
      );
    } catch {
      pushToast('danger', t('Could not copy to the clipboard.'));
    } finally {
      setCopying(false);
    }
  };

  const openLink = (kind: GameNodeLink) => {
    void commands.openGameNodeLink(kind).catch(() => undefined);
  };

  return (
    <>
      <SettingsPanel
        title={t('settings.nav.about')}
        description={t('settings.about.description')}
      >
        <section className="flex flex-wrap items-center gap-[18px] rounded-card border border-border-subtle bg-secondary p-lg">
          <img
            src={wordmark}
            width={83}
            height={32}
            alt="GameNode"
            draggable={false}
            className="shrink-0"
          />
          <div className="flex min-w-0 flex-1 flex-col gap-[4px]">
            <span className="text-md font-semibold text-text-primary">
              {t('settings.about.versionLine', { version })}
            </span>
            <span className="text-xs text-text-dimmed">{buildLine}</span>
          </div>
        </section>

        <SettingsCard title={t('settings.about.diagnostics')}>
          <SettingsRow
            icon={IconTerminal2}
            label={t('settings.about.recentLog')}
            help={t('settings.about.recentLogHelp')}
            control={
              <div className="flex flex-wrap items-center gap-compact">
                <Button
                  size="sm"
                  variant="secondary"
                  leftIcon={IconFileText}
                  disabled={!logDirectory}
                  onClick={() => setLogOpen(true)}
                >
                  {t('settings.about.showRecentLog')}
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  leftIcon={IconFolderOpen}
                  disabled={!logDirectory}
                  onClick={() => void commands.openLogsFolder()}
                >
                  {t('settings.about.openLogFolder')}
                </Button>
              </div>
            }
          />
          <SettingsRow
            icon={IconCpu}
            label={t('settings.about.systemInfo')}
            help={t('settings.about.systemInfoHelp')}
            control={
              <Button
                size="sm"
                variant="secondary"
                leftIcon={IconCopy}
                loading={copying}
                onClick={() => void handleCopySystemInfo()}
              >
                {t('settings.about.copy')}
              </Button>
            }
          />
        </SettingsCard>

        <SettingsCard title={t('settings.about.licenses')}>
          <SettingsRow
            icon={IconFileText}
            label={t('settings.about.licenseNotices')}
            help={`${t('GameNode artwork: GPL-3.0-only · Roboto: SIL Open Font License 1.1')}. ${t(
              'settings.about.licensesHelp',
            )}`}
            control={
              <Button
                ref={showLicenseNoticesButtonRef}
                size="sm"
                variant="secondary"
                leftIcon={IconFileText}
                onClick={handleOpenLicenseModal}
              >
                {t('settings.about.showLicenseNotices')}
              </Button>
            }
          />
        </SettingsCard>

        <div className="flex flex-wrap items-center gap-lg px-[4px]">
          {ABOUT_LINKS.map((link) => (
            <button
              key={link.kind}
              type="button"
              className="inline-flex items-center gap-[4px] rounded-sm text-xs font-medium text-brand transition-colors hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
              onClick={() => openLink(link.kind)}
            >
              <span>{t(link.labelKey)}</span>
              <IconExternalLink size={12} stroke={2} aria-hidden="true" />
            </button>
          ))}
        </div>
      </SettingsPanel>

      {logOpen ? <RecentLogModal onClose={() => setLogOpen(false)} /> : null}
    </>
  );
};
