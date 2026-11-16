import React, { useEffect, useState } from 'react';
import {
  IconArchive,
  IconArrowBackUp,
  IconChevronDown,
  IconDatabase,
  IconFolder,
  IconFolderOpen,
  IconHistory,
  IconShieldCheck,
  IconShieldOff,
} from '@tabler/icons-react';
import { DialogShell } from '../../components/dialog';
import {
  Button,
  Field,
  InsetCard,
  Pill,
  SectionLabel,
  SelectField,
  Switch,
  TextInput,
  type PillTone,
} from '../../components/ui';
import {
  commands,
  useLocalGames,
  type BackupVersion,
  type JobState,
  type SaveSupport,
} from '../../api';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { formatDuration } from '../library/formatters';

export interface SaveProtectionModalProps {
  installationId?: string | null;
  onClose?: () => void;
}

export function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

const RETENTION_OPTIONS = [5, 10, 20, 50] as const;
const DAY_MS = 24 * 60 * 60 * 1000;

function startOfLocalDay(seconds: number): number {
  const date = new Date(seconds * 1000);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

function backupTimeLabel(
  version: BackupVersion,
  nowSeconds: number,
  locale: string,
  t: (key: string, params?: Record<string, string | number>) => string,
): string {
  if (version.createdAt == null) return version.label;
  const created = version.createdAt;
  const days = Math.round(
    (startOfLocalDay(nowSeconds) - startOfLocalDay(created)) / DAY_MS,
  );
  const time = new Intl.DateTimeFormat(locale, {
    hour: '2-digit',
    minute: '2-digit',
  }).format(new Date(created * 1000));
  if (days <= 0) return t('drmFree.todayAt', { time });
  if (days === 1) return t('drmFree.yesterdayAt', { time });
  const date = new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'short',
  }).format(new Date(created * 1000));
  return t('drmFree.dateAt', { date, time });
}

function versionKey(version: BackupVersion): string {
  return `${version.profile.length}:${version.profile}:${version.id.length}:${version.id}:${version.destination}`;
}

function reasonLabel(
  version: BackupVersion,
  t: (key: string, params?: Record<string, string | number>) => string,
): string | null {
  switch (version.reason) {
    case 'afterSession':
      return version.playSeconds != null
        ? t('drmFree.afterPlaySessionDuration', {
            duration: formatDuration(version.playSeconds, t),
          })
        : t('drmFree.afterPlaySession');
    case 'manual':
      return t('drmFree.manualBackup');
    case 'recovery':
      return t('drmFree.recoveryBackup');
    default:
      return null;
  }
}

function reasonIcon(version: BackupVersion) {
  switch (version.reason) {
    case 'manual':
      return { Icon: IconArchive, className: 'text-text-secondary' };
    case 'recovery':
      return { Icon: IconShieldCheck, className: 'text-warning' };
    default:
      return { Icon: IconHistory, className: 'text-text-secondary' };
  }
}

function supportLabel(
  support: SaveSupport | undefined,
  t: (key: string) => string,
): string {
  switch (support) {
    case 'supported':
      return t('drmFree.supportSupported');
    case 'needsSelection':
      return t('drmFree.supportNeedsSelection');
    case 'unsupported':
      return t('drmFree.supportUnsupported');
    case 'noSaves':
      return t('drmFree.supportNoSaves');
    default:
      return t('drmFree.supportNotConfigured');
  }
}

function supportTone(support: SaveSupport | undefined): PillTone {
  if (support === 'supported') return 'success';
  if (support === 'unsupported') return 'danger';
  if (support === 'needsSelection') return 'warning';
  return 'neutral';
}

function jobLabel(
  state: JobState,
  t: (key: string) => string,
): string {
  switch (state) {
    case 'queued':
      return t('drmFree.queued');
    case 'running':
      return t('drmFree.running');
    case 'unchanged':
      return t('drmFree.unchanged');
    case 'successful':
      return t('drmFree.backupCompleted');
    case 'partial':
      return t('drmFree.partialFailure');
    case 'failed':
      return t('drmFree.backupFailed');
    case 'cancelled':
      return t('drmFree.cancelled');
    default:
      return state;
  }
}

function jobTone(state: JobState): PillTone {
  if (state === 'successful' || state === 'unchanged') return 'success';
  if (state === 'failed' || state === 'partial') return 'danger';
  if (state === 'queued' || state === 'running') return 'brand';
  return 'neutral';
}

export const SaveProtectionModal: React.FC<SaveProtectionModalProps> = ({
  installationId: propInstallationId,
  onClose,
}) => {
  const { t, locale } = useI18n();
  const localGames = useLocalGames();
  const saveProtectionEntryKey = useUiStore((s) => s.saveProtectionEntryKey);

  const effectiveKey = propInstallationId ?? saveProtectionEntryKey;
  const isOpen = effectiveKey != null;

  const id = effectiveKey ? effectiveKey.replace(/^manual:/, '') : '';
  const registration = localGames.registrations.find(
    (r) => r.id === id || r.id === effectiveKey || `manual:${r.id}` === effectiveKey,
  );
  // The engine treats the most recently saved profile as the active profile.
  const profile = localGames.profiles.filter((p) => p.installation === id).at(-1);
  const profileJobs = localGames.jobs.filter(
    (j) => j.installation === id && j.profile === profile?.id,
  );
  const job =
    profileJobs.find((j) => j.state === 'running') ??
    profileJobs.find((j) => j.state === 'queued') ??
    profileJobs.sort((a, b) => b.readyAt - a.readyAt)[0];
  const versions = localGames.versions
    .filter((v) => v.installation === id)
    .slice()
    .sort((a, b) => (b.createdAt ?? 0) - (a.createdAt ?? 0));

  const title = registration?.title ?? profile?.title ?? id;

  const [customLocation, setCustomLocation] = useState('');
  const [profileTitleInput, setProfileTitleInput] = useState('');
  const [filesOpen, setFilesOpen] = useState(false);

  useEffect(() => {
    setFilesOpen(false);
    setCustomLocation('');
  }, [effectiveKey]);

  useEffect(() => {
    if (profile?.title) {
      setProfileTitleInput(profile.title);
    }
  }, [profile?.title]);

  const handleClose = () => {
    if (localGames.restore) {
      commands.cancelLocalRestore();
    } else {
      uiStore.setState((s) => ({ ...s, saveProtectionEntryKey: null }));
      onClose?.();
    }
  };

  if (!isOpen || !effectiveKey) {
    return null;
  }

  // Check for overlapping saves across different profiles
  const hasOverlappingSaves = Boolean(
    profile &&
      localGames.profiles.some(
        (p) =>
          p.installation !== id &&
          ((profile.title && p.title.toLowerCase() === profile.title.toLowerCase()) ||
            p.files.some((f) => profile.files.includes(f))),
      ),
  );

  const isAutoBackupChecked =
    profile?.automatic !== undefined && profile?.automatic !== null
      ? profile.automatic
      : localGames.settings.automatic;

  const configuredRetention =
    profile?.retention &&
    RETENTION_OPTIONS.includes(profile.retention as (typeof RETENTION_OPTIONS)[number])
      ? profile.retention
      : RETENTION_OPTIONS.includes(localGames.settings.retention as (typeof RETENTION_OPTIONS)[number])
        ? localGames.settings.retention
        : 10;

  const historyBytes = versions.reduce(
    (total, version) => total + version.bytes,
    0,
  );
  const historyCount = versions.length;
  const saveFolder = profile?.files[0] ?? t('drmFree.noSaveLocationsResolvedYet');
  const backupFolder = versions[0]?.destination ?? localGames.settings.destination;
  const busy = job?.state === 'running' || job?.state === 'queued';
  const nowSeconds = Math.floor(Date.now() / 1000);

  return (
    <DialogShell
      title={t('drmFree.localSaveProtection')}
      subtitle={`${title} · ${t('Detail::DRM-free')}`}
      geometryKey="save-protection"
      defaultSize={{ width: 580, height: 700 }}
      minSize={{ width: 480, height: 480 }}
      bodyClassName="[&>*]:shrink-0"
      onClose={handleClose}
      testId="save-protection-modal"
      footerLeading={
        <>
          <Button
            size="sm"
            variant="secondary"
            leftIcon={IconArchive}
            data-testid="backup-local-now-button"
            disabled={!profile || busy || Boolean(localGames.restore)}
            onClick={() => commands.backupLocalNow(id)}
          >
            {t('drmFree.backUpNow')}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            leftIcon={IconFolderOpen}
            disabled={!backupFolder}
            onClick={() => {
              if (backupFolder) void commands.openInstallFolder(backupFolder);
            }}
          >
            {t('drmFree.openFolder')}
          </Button>
        </>
      }
      footer={
        <Button size="sm" variant="primary" onClick={handleClose}>
          {t('drmFree.close')}
        </Button>
      }
    >
      {localGames.restore ? (
        <div
          data-testid="restore-confirmation-view"
          className="flex flex-col gap-md rounded-md border border-warning bg-warning-subtle p-md"
        >
          <div>
            <h3 className="text-body font-semibold text-warning">
              {t('drmFree.restorePreview')}
            </h3>
            <p className="mt-1 text-xs leading-note text-text-secondary">
              {t('drmFree.restoreReplacesTheseTargetsAfterPreservingA')}
            </p>
          </div>

          <div className="max-h-[160px] overflow-y-auto rounded-control border border-border-subtle bg-body p-sm font-mono text-xs text-text-default">
            {localGames.restore.targets.length === 0 ? (
              <span className="text-text-dimmed">
                {t('drmFree.noSaveFilesExistOnDiskYet')}
              </span>
            ) : (
              localGames.restore.targets.map((target, index) => (
                <div key={`${target}-${index}`} className="break-all py-0.5">
                  {target}
                </div>
              ))
            )}
          </div>

          <div className="flex justify-end gap-compact">
            <Button
              size="sm"
              variant="secondary"
              onClick={() => commands.cancelLocalRestore()}
            >
              {t('drmFree.cancel')}
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() => commands.confirmLocalRestore()}
            >
              {t('drmFree.confirmRestore')}
            </Button>
          </div>
        </div>
      ) : (
        <>
          {hasOverlappingSaves && (
            <div
              data-testid="overlapping-saves-banner"
              className="rounded-md border border-warning bg-warning-subtle px-sm py-compact text-xs leading-note text-text-default"
            >
              {t('drmFree.anotherInstallationSharesTheseSavesBackupAnd')}
            </div>
          )}

          <InsetCard
            icon={isAutoBackupChecked ? IconShieldCheck : IconShieldOff}
            iconClassName={isAutoBackupChecked ? 'text-success' : 'text-text-muted'}
            title={
              isAutoBackupChecked
                ? t('drmFree.protectionOn')
                : t('drmFree.protectionOff')
            }
            description={
              isAutoBackupChecked
                ? t('drmFree.protectionOnDescription')
                : t('drmFree.protectionOffDescription')
            }
            trailing={
              <div className="flex items-center gap-sm">
                <span data-testid="save-support-badge">
                  <Pill tone={supportTone(profile?.support)}>
                    {supportLabel(profile?.support, t)}
                  </Pill>
                </span>
                <Switch
                  aria-label={t('drmFree.protectionOn')}
                  data-testid="automatic-backup-toggle"
                  checked={isAutoBackupChecked}
                  disabled={!profile}
                  onChange={(event) =>
                    commands.setLocalProfileAutomatic(
                      id,
                      event.currentTarget.checked,
                    )
                  }
                />
              </div>
            }
          />

          <Field label={t('drmFree.saveFolder')}>
            <div className="flex items-end gap-compact">
              <TextInput
                readOnly
                leadingIcon={IconFolder}
                value={saveFolder}
                aria-label={t('drmFree.saveFolder')}
              />
              <Button
                size="md"
                variant="secondary"
                disabled={!profile}
                onClick={() => commands.pickLocalSaves(id, true)}
              >
                {t('drmFree.change')}
              </Button>
            </div>
          </Field>

          <div className="grid grid-cols-2 gap-md">
            <Field label={t('drmFree.keep')}>
              <SelectField
                aria-label={t('drmFree.keep')}
                value={String(configuredRetention)}
                disabled={!profile}
                onChange={(value) =>
                  commands.setLocalBackupRetention(effectiveKey, Number(value))
                }
                options={RETENTION_OPTIONS.map((value) => ({
                  value: String(value),
                  label: t('drmFree.mostRecentCopies', { n: value }),
                }))}
              />
            </Field>
            <Field label={t('drmFree.spaceUsed')}>
              <TextInput
                readOnly
                leadingIcon={IconDatabase}
                aria-label={t('drmFree.spaceUsed')}
                value={t('drmFree.spaceUsedValue', {
                  size: formatBytes(historyBytes),
                  n: historyCount,
                })}
              />
            </Field>
          </div>

          <div className="flex flex-col gap-1">
            <SectionLabel>{t('drmFree.backupHistory')}</SectionLabel>
            <p className="text-xs leading-note text-text-dimmed">
              {t('drmFree.backupHistoryDescription')}
            </p>
            {job && (
              <div className="flex items-center gap-compact pt-1">
                <span data-testid="backup-job-state-badge">
                  <Pill tone={jobTone(job.state)}>{jobLabel(job.state, t)}</Pill>
                </span>
                {job.error && (
                  <span className="min-w-0 flex-1 truncate text-xs text-danger">
                    {job.error}
                  </span>
                )}
              </div>
            )}
          </div>

          <div
            data-testid="backup-history"
            className="shrink-0 overflow-hidden rounded-md border border-border-subtle"
          >
            {versions.length === 0 ? (
              <div className="p-md text-center text-xs text-text-dimmed">
                {t('drmFree.noCopiesYet')}
              </div>
            ) : (
              versions.map((version) => {
                const { Icon, className } = reasonIcon(version);
                const reason = reasonLabel(version, t);
                return (
                  <div
                    key={`${version.profile}:${version.id}:${version.destination}`}
                    data-testid={`backup-version-${version.id}`}
                    data-reason={version.reason ?? 'unknown'}
                    className="flex items-center gap-sm border-b border-border-subtle px-sm py-[10px] last:border-b-0"
                  >
                    <Icon
                      size={16}
                      stroke={2}
                      aria-hidden="true"
                      className={`shrink-0 ${className}`}
                    />
                    <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                      <span className="truncate text-body font-medium text-text-primary">
                        {backupTimeLabel(version, nowSeconds, locale, t)}
                      </span>
                      {reason && (
                        <span className="truncate text-[11px] text-text-dimmed">
                          {reason}
                        </span>
                      )}
                    </div>
                    <span className="shrink-0 text-xs text-text-dimmed">
                      {formatBytes(version.bytes)}
                    </span>
                    <Button
                      size="sm"
                      variant="secondary"
                      leftIcon={IconArrowBackUp}
                      onClick={() => commands.previewLocalRestore(versionKey(version))}
                    >
                      {t('drmFree.restore')}
                    </Button>
                  </div>
                );
              })
            )}
          </div>

          <div className="flex flex-col gap-compact">
            <button
              type="button"
              aria-expanded={filesOpen}
              data-testid="protected-files-toggle"
              onClick={() => setFilesOpen((open) => !open)}
              className="flex items-center gap-[6px] self-start text-xs font-medium text-text-secondary transition-colors hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
            >
              <IconChevronDown
                size={14}
                stroke={2}
                aria-hidden="true"
                className={`text-text-dimmed transition-transform ${filesOpen ? '' : '-rotate-90'}`}
              />
              {t('drmFree.protectedFiles')}
            </button>

            {filesOpen && (
              <div className="flex flex-col gap-sm rounded-md border border-border-subtle bg-body p-sm">
                <p className="text-xs leading-note text-text-dimmed">
                  {t('drmFree.protectedFilesDescription')}
                </p>
                <div className="flex flex-col gap-1">
                  <SectionLabel>{t('drmFree.saveFoldersFiles')}</SectionLabel>
                  {profile && profile.files.length > 0 ? (
                    profile.files.map((file) => (
                      <div
                        key={file}
                        className="break-all rounded-control border border-border-subtle bg-surface px-xs py-1 text-xs text-text-default"
                      >
                        {file}
                      </div>
                    ))
                  ) : (
                    <span className="text-xs text-text-dimmed">
                      {t('drmFree.noSaveLocationsResolvedYet')}
                    </span>
                  )}
                </div>
                {profile && profile.registry.length > 0 && (
                  <div className="flex flex-col gap-1">
                    <SectionLabel>{t('drmFree.registryKeys')}</SectionLabel>
                    {profile.registry.map((key) => (
                      <div
                        key={key}
                        className="break-all rounded-control border border-border-subtle bg-surface px-xs py-1 text-xs text-text-dimmed"
                      >
                        {key}
                      </div>
                    ))}
                  </div>
                )}

                <div className="flex flex-wrap gap-compact">
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={!profile}
                    onClick={() => commands.pickLocalSaves(id, true)}
                  >
                    {t('drmFree.changeSaveFolder')}
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={!profile}
                    onClick={() => commands.pickLocalSaves(id, false)}
                  >
                    {t('drmFree.changeSaveFile')}
                  </Button>
                </div>

                <SectionLabel>{t('drmFree.saveLocationProfileOverride')}</SectionLabel>

                <Field label={t('drmFree.savePathPlaceholder')}>
                  <div className="flex items-end gap-compact">
                    <TextInput
                      placeholder={t('drmFree.savePathPlaceholder')}
                      aria-label={t('drmFree.savePathPlaceholder')}
                      value={customLocation}
                      onChange={(event) => setCustomLocation(event.target.value)}
                    />
                    <Button
                      size="md"
                      variant="secondary"
                      disabled={!profile || !customLocation.trim()}
                      onClick={() =>
                        commands.configureLocalSaves(id, customLocation)
                      }
                    >
                      {t('drmFree.useThisSaveLocation')}
                    </Button>
                  </div>
                </Field>

                <Field label={t('drmFree.saveProfile')}>
                  <div className="flex items-end gap-compact">
                    <TextInput
                      placeholder={t('drmFree.saveProfileTitlePlaceholder')}
                      aria-label={t('drmFree.saveProfileTitlePlaceholder')}
                      value={profileTitleInput}
                      onChange={(event) =>
                        setProfileTitleInput(event.target.value)
                      }
                    />
                    <Button
                      size="md"
                      variant="secondary"
                      disabled={!profile || !profileTitleInput.trim()}
                      onClick={() =>
                        commands.chooseLocalSaveProfile(id, profileTitleInput)
                      }
                    >
                      {t('drmFree.changeSaveProfile')}
                    </Button>
                  </div>
                </Field>
              </div>
            )}
          </div>
        </>
      )}
    </DialogShell>
  );
};
