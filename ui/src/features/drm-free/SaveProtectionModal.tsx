import React, { useState, useEffect } from 'react';
import { IconX } from '@tabler/icons-react';
import { FloatingModal } from '../../components/FloatingModal';
import { commands, useLocalGames, type SaveSupport } from '../../api';
import { uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';

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

export function formatSaveSupport(support?: SaveSupport): string {
  switch (support) {
    case 'supported':
      return 'Supported';
    case 'needsSelection':
      return 'Needs selection';
    case 'unsupported':
      return 'Unsupported';
    case 'noSaves':
      return 'Supported · No saves found yet';
    default:
      return 'Not configured';
  }
}

const sectionCardClass =
  'flex flex-col gap-sm rounded-md border border-border-subtle bg-secondary p-md';
const smallButtonClass =
  'inline-flex items-center justify-center whitespace-nowrap rounded-sm border border-border-subtle bg-raised px-3.5 py-compact text-xs font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-50';
const pathRowClass =
  'break-all rounded-sm border border-border-subtle bg-chrome px-2.5 py-1.5 text-xs';
const smallInputClass =
  'min-w-0 flex-1 rounded-sm border border-border-subtle bg-chrome px-sm py-compact text-xs text-text-default placeholder:text-text-dimmed focus:border-brand focus:outline-none focus:ring-1 focus:ring-brand';

export const SaveProtectionModal: React.FC<SaveProtectionModalProps> = ({
  installationId: propInstallationId,
  onClose,
}) => {
  const { t } = useI18n();
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
  const versions = localGames.versions.filter((v) => v.installation === id);

  const title = registration?.title ?? profile?.title ?? id;

  const [customLocation, setCustomLocation] = useState('');
  const [profileTitleInput, setProfileTitleInput] = useState('');

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

  const supportToneClass =
    profile?.support === 'supported'
      ? 'bg-success-subtle text-success'
      : profile?.support === 'unsupported'
        ? 'bg-danger-subtle text-danger'
        : 'bg-warning-subtle text-warning';

  const jobToneClass =
    job?.state === 'successful' || job?.state === 'unchanged'
      ? 'bg-success-subtle text-success'
      : job?.state === 'failed'
        ? 'bg-danger-subtle text-danger'
        : 'bg-accent-subtle text-brand';

  return (
    <FloatingModal.Root
      opened
      onClose={handleClose}
      geometryKey="save-protection"
      defaultSize={{ width: 720, height: 600 }}
      minSize={{ width: 560, height: 420 }}
      zIndex={1000}
      closeOnClickOutside
    >
      <FloatingModal.Overlay backgroundOpacity={0.62} />
      <FloatingModal.Content className="flex flex-col bg-chrome">
        {/* Header */}
        <FloatingModal.Header className="bg-chrome px-control-inline py-md">
          <div>
            <FloatingModal.Title className="text-lg font-semibold text-text-default">
              {t('drmFree.localSaveProtection')}
            </FloatingModal.Title>
            <div className="text-xs text-text-dimmed">{title}</div>
          </div>
          <FloatingModal.CloseButton
            aria-label="Close"
            icon={<IconX size={18} stroke={1.5} />}
            className="rounded-sm text-text-dimmed transition-colors hover:bg-raised hover:text-text-default"
          />
        </FloatingModal.Header>

        {/* Content */}
        <FloatingModal.Body className="gap-lg p-control-inline">
          {/* Two-step Restore Confirmation View */}
          {localGames.restore ? (
            <div
              data-testid="restore-confirmation-view"
              className="flex flex-col gap-md rounded-md border border-warning bg-secondary p-lg"
            >
              <div>
                <h3 className="text-md text-warning">{t('drmFree.restorePreview')}</h3>
                <p className="mt-1 text-sm text-text-default">
                  {t('drmFree.restoreReplacesTheseTargetsAfterPreservingA')}
                </p>
              </div>

              <div className="max-h-[160px] overflow-y-auto rounded-sm border border-border-subtle bg-chrome p-sm font-mono text-xs">
                {localGames.restore.targets.length === 0 ? (
                  <span className="text-text-dimmed">No files will be modified.</span>
                ) : (
                  localGames.restore.targets.map((tgt, i) => (
                    <div key={i} className="break-all py-0.5">
                      {tgt}
                    </div>
                  ))
                )}
              </div>

              <div className="flex justify-end gap-compact">
                <button
                  type="button"
                  onClick={() => commands.cancelLocalRestore()}
                  className="inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                >
                  {t('drmFree.cancel')}
                </button>
                <button
                  type="button"
                  onClick={() => commands.confirmLocalRestore()}
                  className="inline-flex items-center justify-center rounded-sm border border-transparent bg-danger px-md py-compact text-sm font-semibold text-white transition-colors hover:opacity-90 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-danger"
                >
                  {t('drmFree.confirmRestore')}
                </button>
              </div>
            </div>
          ) : (
            <>
              {/* Overlapping saves warning banner */}
              {hasOverlappingSaves && (
                <div
                  data-testid="overlapping-saves-banner"
                  className="rounded-sm border border-warning bg-warning-subtle px-md py-sm text-sm text-text-default"
                >
                  {t('drmFree.anotherInstallationSharesTheseSavesBackupAnd')}
                </div>
              )}

              {/* Profile summary card */}
              <div className={sectionCardClass}>
                <div className="flex items-center justify-between gap-sm">
                  <div>
                    <div className="text-md font-semibold text-text-default">
                      {profile?.title || t('drmFree.noSaveProfileChooseASaveLocation')}
                    </div>
                    <div data-testid="save-support-text" className="text-xs text-text-dimmed">
                      Support: {formatSaveSupport(profile?.support)}
                    </div>
                  </div>
                  <span
                    data-testid="save-support-badge"
                    className={`rounded-sm px-2.5 py-1 text-xs font-semibold ${supportToneClass}`}
                  >
                    {profile?.support ?? 'not configured'}
                  </span>
                </div>

                {/* Backed-up files */}
                <div>
                  <div className="mb-1.5 text-xs font-semibold text-text-dimmed">
                    {t('drmFree.saveFoldersFiles')}:
                  </div>
                  {profile && profile.files.length > 0 ? (
                    <div className="flex flex-col gap-1">
                      {profile.files.map((file, i) => (
                        <div key={i} className={`${pathRowClass} text-text-default`}>
                          {file}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div className="text-xs text-text-dimmed">
                      {t('drmFree.noSaveLocationsResolvedYet')}
                    </div>
                  )}
                </div>

                {/* Backed-up registry keys */}
                {profile && profile.registry.length > 0 && (
                  <div>
                    <div className="mb-1.5 text-xs font-semibold text-text-dimmed">
                      {t('drmFree.registryKeys')}:
                    </div>
                    <div className="flex flex-col gap-1">
                      {profile.registry.map((reg, i) => (
                        <div key={i} className={`${pathRowClass} text-text-dimmed`}>
                          {reg}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>

              {/* Automatic backup toggle */}
              <div className="flex items-center justify-between gap-md rounded-md border border-border-subtle bg-secondary px-md py-3.5">
                <div>
                  <div className="text-sm font-semibold text-text-default">
                    {t('drmFree.backUpThisGameAutomatically')}
                  </div>
                  <div className="text-xs text-text-dimmed">
                    {t('drmFree.createsALocalSnapshotAfterEachPlay')}
                  </div>
                </div>
                <input
                  type="checkbox"
                  aria-label="Back up this game automatically"
                  data-testid="automatic-backup-toggle"
                  checked={isAutoBackupChecked}
                  onChange={(e) => commands.setLocalProfileAutomatic(id, e.target.checked)}
                  className="h-4 w-4 cursor-pointer accent-brand"
                />
              </div>

              {/* Custom save path & profile override */}
              <div className={sectionCardClass}>
                <div className="text-sm font-semibold text-text-default">
                  {t('drmFree.saveLocationProfileOverride')}
                </div>

                <div className="flex gap-compact">
                  <button
                    type="button"
                    onClick={() => commands.pickLocalSaves(id, true)}
                    className={smallButtonClass}
                  >
                    Browse folder
                  </button>
                  <button
                    type="button"
                    onClick={() => commands.pickLocalSaves(id, false)}
                    className={smallButtonClass}
                  >
                    Browse file
                  </button>
                </div>

                {/* Custom location input */}
                <div className="flex gap-compact">
                  <input
                    type="text"
                    placeholder="Custom save file or folder"
                    aria-label="Custom save path"
                    value={customLocation}
                    onChange={(e) => setCustomLocation(e.target.value)}
                    className={smallInputClass}
                  />
                  <button
                    type="button"
                    onClick={() => commands.configureLocalSaves(id, customLocation)}
                    disabled={!customLocation.trim()}
                    className={smallButtonClass}
                  >
                    Save path
                  </button>
                </div>

                {/* Profile title override */}
                <div className="flex gap-compact">
                  <input
                    type="text"
                    placeholder="Exact save profile title"
                    aria-label="Save profile title"
                    value={profileTitleInput}
                    onChange={(e) => setProfileTitleInput(e.target.value)}
                    className={smallInputClass}
                  />
                  <button
                    type="button"
                    onClick={() => commands.chooseLocalSaveProfile(id, profileTitleInput)}
                    disabled={!profileTitleInput.trim()}
                    className={smallButtonClass}
                  >
                    {t('drmFree.changeSaveProfile')}
                  </button>
                </div>
              </div>

              {/* Backup Now & Job Status */}
              <div className="flex items-center justify-between gap-md rounded-md border border-border-subtle bg-secondary px-md py-3.5">
                <div>
                  <div className="text-sm font-semibold text-text-default">Immediate backup</div>
                  {job && (
                    <div className="mt-1 flex items-center gap-compact">
                      <span
                        data-testid="backup-job-state-badge"
                        className={`rounded-sm px-2 py-0.5 text-xs font-semibold ${jobToneClass}`}
                      >
                        {job.state}
                      </span>
                      {job.error && (
                        <span className="text-xs text-danger">{job.error}</span>
                      )}
                    </div>
                  )}
                </div>

                <button
                  type="button"
                  data-testid="backup-local-now-button"
                  onClick={() => commands.backupLocalNow(id)}
                  disabled={job?.state === 'running' || job?.state === 'queued'}
                  className="inline-flex items-center justify-center rounded-sm border border-transparent bg-brand px-md py-compact text-sm font-semibold text-on-primary transition-colors hover:bg-brand-4 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand disabled:cursor-not-allowed disabled:opacity-60"
                >
                  {t('drmFree.backupNow')}
                </button>
              </div>

              {/* Backup History */}
              <div className={sectionCardClass}>
                <div className="flex items-center justify-between gap-sm">
                  <div className="text-sm font-semibold text-text-default">Backup history</div>
                  <button
                    type="button"
                    onClick={() => commands.loadLocalHistory(id)}
                    className="inline-flex cursor-pointer items-center justify-center rounded-sm border border-border-subtle bg-transparent px-2.5 py-1 text-xs text-text-dimmed transition-colors hover:bg-raised hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                  >
                    Refresh
                  </button>
                </div>

                {versions.length === 0 ? (
                  <div className="p-md text-center text-xs text-text-dimmed">
                    No backups recorded yet.
                  </div>
                ) : (
                  <div className="flex flex-col gap-compact">
                    {versions.map((ver) => {
                      const versionKey = `${ver.profile.length}:${ver.profile}:${ver.id.length}:${ver.id}:${ver.destination}`;

                      return (
                        <div
                          key={ver.id}
                          data-testid={`backup-version-${ver.id}`}
                          className="flex items-center justify-between gap-sm rounded-sm border border-border-subtle bg-chrome px-3.5 py-2.5"
                        >
                          <div>
                            <div className="text-sm font-medium text-text-default">{ver.label}</div>
                            <div className="text-xs text-text-dimmed">
                              {formatBytes(ver.bytes)} · {ver.destination}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() => commands.previewLocalRestore(versionKey)}
                            className="inline-flex cursor-pointer items-center justify-center whitespace-nowrap rounded-sm border border-border-subtle bg-raised px-3.5 py-1.5 text-xs text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                          >
                            Restore
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            </>
          )}
        </FloatingModal.Body>

        {/* Footer */}
        <FloatingModal.Footer className="px-control-inline py-md">
          <button
            type="button"
            onClick={handleClose}
            className="inline-flex items-center justify-center rounded-sm border border-border-subtle bg-raised px-md py-compact text-sm font-medium text-text-default transition-colors hover:bg-surface focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
          >
            {t('drmFree.close')}
          </button>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
