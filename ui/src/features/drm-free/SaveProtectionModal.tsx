import React, { useState, useEffect } from 'react';
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

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        handleClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isOpen, localGames.restore]);

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

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="save-protection-heading"
      style={{
        position: 'fixed',
        inset: 0,
        backgroundColor: 'var(--settings-overlay, rgba(0, 0, 0, 0.62))',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        zIndex: 1000,
        padding: 'var(--modal-margin, 24px)',
      }}
      onClick={(e) => {
        if (e.target === e.currentTarget) handleClose();
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: '720px',
          maxHeight: '90vh',
          backgroundColor: 'var(--settings-bg-base, #111111)',
          border: '1px solid var(--settings-border-subtle, #2A2A2A)',
          borderRadius: 'var(--radius-md, 8px)',
          boxShadow: 'var(--shadow-modal)',
          display: 'flex',
          flexDirection: 'column',
          overflow: 'hidden',
        }}
      >
        {/* Header */}
        <div
          style={{
            padding: '16px 24px',
            borderBottom: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
          }}
        >
          <div>
            <h2 id="save-protection-heading" style={{ fontSize: 'var(--font-size-lg, 18px)', fontWeight: 600 }}>
              {t('drmFree.localSaveProtection')}
            </h2>
            <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
              {title}
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={handleClose}
            style={{
              padding: '6px 12px',
              backgroundColor: 'transparent',
              color: 'var(--color-text-dimmed, #808080)',
              borderRadius: 'var(--radius-sm, 4px)',
              cursor: 'pointer',
            }}
          >
            ✕
          </button>
        </div>

        {/* Content */}
        <div
          style={{
            padding: '24px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '20px',
          }}
        >
          {/* Two-step Restore Confirmation View */}
          {localGames.restore ? (
            <div
              data-testid="restore-confirmation-view"
              style={{
                backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                border: '1px solid var(--color-warning, #F5A524)',
                borderRadius: 'var(--radius-md, 8px)',
                padding: '20px',
                display: 'flex',
                flexDirection: 'column',
                gap: '16px',
              }}
            >
              <div>
                <h3 style={{ fontSize: 'var(--font-size-md, 16px)', color: 'var(--color-warning, #F5A524)' }}>
                  {t('drmFree.restorePreview')}
                </h3>
                <p style={{ fontSize: 'var(--font-size-sm, 14px)', color: 'var(--color-text-default, #D9D9D9)', marginTop: '4px' }}>
                  {t('drmFree.restoreReplacesTheseTargetsAfterPreservingA')}
                </p>
              </div>

              <div
                style={{
                  backgroundColor: 'var(--settings-bg-base, #111111)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-sm, 4px)',
                  padding: '12px',
                  maxHeight: '160px',
                  overflowY: 'auto',
                  fontFamily: 'monospace',
                  fontSize: 'var(--font-size-xs, 12px)',
                }}
              >
                {localGames.restore.targets.length === 0 ? (
                  <span style={{ color: 'var(--color-text-dimmed, #808080)' }}>No files will be modified.</span>
                ) : (
                  localGames.restore.targets.map((tgt, i) => (
                    <div key={i} style={{ padding: '2px 0', wordBreak: 'break-all' }}>
                      {tgt}
                    </div>
                  ))
                )}
              </div>

              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: '8px' }}>
                <button
                  type="button"
                  onClick={() => commands.cancelLocalRestore()}
                  style={{
                    padding: '8px 16px',
                    backgroundColor: 'var(--settings-bg-raised, #222222)',
                    border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    color: 'var(--color-text-default, #D9D9D9)',
                    cursor: 'pointer',
                  }}
                >
                  {t('drmFree.cancel')}
                </button>
                <button
                  type="button"
                  onClick={() => commands.confirmLocalRestore()}
                  style={{
                    padding: '8px 20px',
                    backgroundColor: 'var(--color-danger, #E5484D)',
                    border: 'none',
                    borderRadius: 'var(--radius-sm, 4px)',
                    color: '#FFFFFF',
                    fontWeight: 600,
                    cursor: 'pointer',
                  }}
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
                  style={{
                    padding: '12px 16px',
                    backgroundColor: 'var(--color-warning-subtle, #F5A5241F)',
                    border: '1px solid var(--color-warning, #F5A524)',
                    borderRadius: 'var(--radius-sm, 4px)',
                    color: 'var(--color-text-default, #D9D9D9)',
                    fontSize: 'var(--font-size-sm, 14px)',
                  }}
                >
                  {t('drmFree.anotherInstallationSharesTheseSavesBackupAnd')}
                </div>
              )}

              {/* Profile summary card */}
              <div
                style={{
                  backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  padding: '16px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '12px',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div>
                    <div style={{ fontWeight: 600, fontSize: 'var(--font-size-md, 16px)' }}>
                      {profile?.title || t('drmFree.noSaveProfileChooseASaveLocation')}
                    </div>
                    <div
                      data-testid="save-support-text"
                      style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}
                    >
                      Support: {formatSaveSupport(profile?.support)}
                    </div>
                  </div>
                  <span
                    data-testid="save-support-badge"
                    style={{
                      padding: '4px 10px',
                      borderRadius: 'var(--radius-sm, 4px)',
                      fontSize: 'var(--font-size-xs, 12px)',
                      backgroundColor:
                        profile?.support === 'supported'
                          ? 'var(--color-success-subtle, #3DD68C1F)'
                          : profile?.support === 'unsupported'
                            ? 'var(--color-danger-subtle, #E5484D1F)'
                            : 'var(--color-warning-subtle, #F5A5241F)',
                      color:
                        profile?.support === 'supported'
                          ? 'var(--color-success, #3DD68C)'
                          : profile?.support === 'unsupported'
                            ? 'var(--color-danger, #E5484D)'
                            : 'var(--color-warning, #F5A524)',
                      fontWeight: 600,
                    }}
                  >
                    {profile?.support ?? 'not configured'}
                  </span>
                </div>

                {/* Backed-up files */}
                <div>
                  <div
                    style={{
                      fontSize: 'var(--font-size-xs, 12px)',
                      fontWeight: 600,
                      color: 'var(--color-text-dimmed, #808080)',
                      marginBottom: '6px',
                    }}
                  >
                    {t('drmFree.saveFoldersFiles')}:
                  </div>
                  {profile && profile.files.length > 0 ? (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                      {profile.files.map((file, i) => (
                        <div
                          key={i}
                          style={{
                            padding: '6px 10px',
                            backgroundColor: 'var(--settings-bg-base, #111111)',
                            border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                            borderRadius: 'var(--radius-sm, 4px)',
                            fontSize: 'var(--font-size-xs, 12px)',
                            wordBreak: 'break-all',
                          }}
                        >
                          {file}
                        </div>
                      ))}
                    </div>
                  ) : (
                    <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
                      {t('drmFree.noSaveLocationsResolvedYet')}
                    </div>
                  )}
                </div>

                {/* Backed-up registry keys */}
                {profile && profile.registry.length > 0 && (
                  <div>
                    <div
                      style={{
                        fontSize: 'var(--font-size-xs, 12px)',
                        fontWeight: 600,
                        color: 'var(--color-text-dimmed, #808080)',
                        marginBottom: '6px',
                      }}
                    >
                      {t('drmFree.registryKeys')}:
                    </div>
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                      {profile.registry.map((reg, i) => (
                        <div
                          key={i}
                          style={{
                            padding: '6px 10px',
                            backgroundColor: 'var(--settings-bg-base, #111111)',
                            border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                            borderRadius: 'var(--radius-sm, 4px)',
                            fontSize: 'var(--font-size-xs, 12px)',
                            color: 'var(--color-text-dimmed, #808080)',
                            wordBreak: 'break-all',
                          }}
                        >
                          {reg}
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>

              {/* Automatic backup toggle */}
              <div
                style={{
                  padding: '14px 16px',
                  backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                }}
              >
                <div>
                  <div style={{ fontWeight: 600, fontSize: 'var(--font-size-sm, 14px)' }}>
                    {t('drmFree.backUpThisGameAutomatically')}
                  </div>
                  <div style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-text-dimmed, #808080)' }}>
                    {t('drmFree.createsALocalSnapshotAfterEachPlay')}
                  </div>
                </div>
                <input
                  type="checkbox"
                  aria-label="Back up this game automatically"
                  data-testid="automatic-backup-toggle"
                  checked={isAutoBackupChecked}
                  onChange={(e) => commands.setLocalProfileAutomatic(id, e.target.checked)}
                  style={{ cursor: 'pointer', transform: 'scale(1.2)' }}
                />
              </div>

              {/* Custom save path & profile override */}
              <div
                style={{
                  backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  padding: '16px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '12px',
                }}
              >
                <div style={{ fontWeight: 600, fontSize: 'var(--font-size-sm, 14px)' }}>
                  {t('drmFree.saveLocationProfileOverride')}
                </div>

                <div style={{ display: 'flex', gap: '8px' }}>
                  <button
                    type="button"
                    onClick={() => commands.pickLocalSaves(id, true)}
                    style={{
                      padding: '8px 14px',
                      backgroundColor: 'var(--settings-bg-raised, #222222)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      color: 'var(--color-text-default, #D9D9D9)',
                      cursor: 'pointer',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  >
                    Browse folder
                  </button>
                  <button
                    type="button"
                    onClick={() => commands.pickLocalSaves(id, false)}
                    style={{
                      padding: '8px 14px',
                      backgroundColor: 'var(--settings-bg-raised, #222222)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      color: 'var(--color-text-default, #D9D9D9)',
                      cursor: 'pointer',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  >
                    Browse file
                  </button>
                </div>

                {/* Custom location input */}
                <div style={{ display: 'flex', gap: '8px' }}>
                  <input
                    type="text"
                    placeholder="Custom save file or folder"
                    aria-label="Custom save path"
                    value={customLocation}
                    onChange={(e) => setCustomLocation(e.target.value)}
                    style={{
                      flex: 1,
                      backgroundColor: 'var(--settings-bg-base, #111111)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      padding: '8px 12px',
                      color: 'var(--color-text-default, #D9D9D9)',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  />
                  <button
                    type="button"
                    onClick={() => commands.configureLocalSaves(id, customLocation)}
                    disabled={!customLocation.trim()}
                    style={{
                      padding: '8px 14px',
                      backgroundColor: 'var(--settings-bg-raised, #222222)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      color: 'var(--color-text-default, #D9D9D9)',
                      cursor: customLocation.trim() ? 'pointer' : 'not-allowed',
                      opacity: customLocation.trim() ? 1 : 0.5,
                      whiteSpace: 'nowrap',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  >
                    Save path
                  </button>
                </div>

                {/* Profile title override */}
                <div style={{ display: 'flex', gap: '8px' }}>
                  <input
                    type="text"
                    placeholder="Exact save profile title"
                    aria-label="Save profile title"
                    value={profileTitleInput}
                    onChange={(e) => setProfileTitleInput(e.target.value)}
                    style={{
                      flex: 1,
                      backgroundColor: 'var(--settings-bg-base, #111111)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      padding: '8px 12px',
                      color: 'var(--color-text-default, #D9D9D9)',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  />
                  <button
                    type="button"
                    onClick={() => commands.chooseLocalSaveProfile(id, profileTitleInput)}
                    disabled={!profileTitleInput.trim()}
                    style={{
                      padding: '8px 14px',
                      backgroundColor: 'var(--settings-bg-raised, #222222)',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      color: 'var(--color-text-default, #D9D9D9)',
                      cursor: profileTitleInput.trim() ? 'pointer' : 'not-allowed',
                      opacity: profileTitleInput.trim() ? 1 : 0.5,
                      whiteSpace: 'nowrap',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  >
                    {t('drmFree.changeSaveProfile')}
                  </button>
                </div>
              </div>

              {/* Backup Now & Job Status */}
              <div
                style={{
                  padding: '14px 16px',
                  backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                }}
              >
                <div>
                  <div style={{ fontWeight: 600, fontSize: 'var(--font-size-sm, 14px)' }}>
                    Immediate backup
                  </div>
                  {job && (
                    <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginTop: '4px' }}>
                      <span
                        data-testid="backup-job-state-badge"
                        style={{
                          padding: '2px 8px',
                          borderRadius: 'var(--radius-sm, 4px)',
                          fontSize: 'var(--font-size-xs, 12px)',
                          backgroundColor:
                            job.state === 'successful' || job.state === 'unchanged'
                              ? 'var(--color-success-subtle, #3DD68C1F)'
                              : job.state === 'failed'
                                ? 'var(--color-danger-subtle, #E5484D1F)'
                                : 'var(--color-accent-subtle, #F150251F)',
                          color:
                            job.state === 'successful' || job.state === 'unchanged'
                              ? 'var(--color-success, #3DD68C)'
                              : job.state === 'failed'
                                ? 'var(--color-danger, #E5484D)'
                                : 'var(--color-primary, #F15025)',
                          fontWeight: 600,
                        }}
                      >
                        {job.state}
                      </span>
                      {job.error && (
                        <span style={{ fontSize: 'var(--font-size-xs, 12px)', color: 'var(--color-danger, #E5484D)' }}>
                          {job.error}
                        </span>
                      )}
                    </div>
                  )}
                </div>

                <button
                  type="button"
                  data-testid="backup-local-now-button"
                  onClick={() => commands.backupLocalNow(id)}
                  disabled={job?.state === 'running' || job?.state === 'queued'}
                  style={{
                    padding: '8px 18px',
                    backgroundColor: 'var(--color-primary, #F15025)',
                    border: 'none',
                    borderRadius: 'var(--radius-sm, 4px)',
                    color: 'var(--color-on-primary, #EDEDED)',
                    fontWeight: 600,
                    cursor: job?.state === 'running' || job?.state === 'queued' ? 'not-allowed' : 'pointer',
                    opacity: job?.state === 'running' || job?.state === 'queued' ? 0.6 : 1,
                  }}
                >
                  {t('drmFree.backupNow')}
                </button>
              </div>

              {/* Backup History */}
              <div
                style={{
                  backgroundColor: 'var(--settings-bg-surface, #1A1A1A)',
                  border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  padding: '16px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '12px',
                }}
              >
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                  <div style={{ fontWeight: 600, fontSize: 'var(--font-size-sm, 14px)' }}>
                    Backup history
                  </div>
                  <button
                    type="button"
                    onClick={() => commands.loadLocalHistory(id)}
                    style={{
                      padding: '4px 10px',
                      backgroundColor: 'transparent',
                      border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                      borderRadius: 'var(--radius-sm, 4px)',
                      color: 'var(--color-text-dimmed, #808080)',
                      fontSize: 'var(--font-size-xs, 12px)',
                      cursor: 'pointer',
                    }}
                  >
                    Refresh
                  </button>
                </div>

                {versions.length === 0 ? (
                  <div
                    style={{
                      padding: '16px',
                      textAlign: 'center',
                      color: 'var(--color-text-dimmed, #808080)',
                      fontSize: 'var(--font-size-xs, 12px)',
                    }}
                  >
                    No backups recorded yet.
                  </div>
                ) : (
                  <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                    {versions.map((ver) => {
                      const versionKey = `${ver.profile.length}:${ver.profile}:${ver.id.length}:${ver.id}:${ver.destination}`;

                      return (
                        <div
                          key={ver.id}
                          data-testid={`backup-version-${ver.id}`}
                          style={{
                            padding: '10px 14px',
                            backgroundColor: 'var(--settings-bg-base, #111111)',
                            border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                            borderRadius: 'var(--radius-sm, 4px)',
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'space-between',
                          }}
                        >
                          <div>
                            <div style={{ fontWeight: 500, fontSize: 'var(--font-size-sm, 14px)' }}>
                              {ver.label}
                            </div>
                            <div
                              style={{
                                fontSize: 'var(--font-size-xs, 12px)',
                                color: 'var(--color-text-dimmed, #808080)',
                              }}
                            >
                              {formatBytes(ver.bytes)} · {ver.destination}
                            </div>
                          </div>
                          <button
                            type="button"
                            onClick={() => commands.previewLocalRestore(versionKey)}
                            style={{
                              padding: '6px 14px',
                              backgroundColor: 'var(--settings-bg-raised, #222222)',
                              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
                              borderRadius: 'var(--radius-sm, 4px)',
                              color: 'var(--color-text-default, #D9D9D9)',
                              cursor: 'pointer',
                              fontSize: 'var(--font-size-xs, 12px)',
                            }}
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
        </div>

        {/* Footer */}
        <div
          style={{
            padding: '16px 24px',
            borderTop: '1px solid var(--settings-border-subtle, #2A2A2A)',
            display: 'flex',
            justifyContent: 'flex-end',
          }}
        >
          <button
            type="button"
            onClick={handleClose}
            style={{
              padding: '8px 20px',
              backgroundColor: 'var(--settings-bg-raised, #222222)',
              border: '1px solid var(--settings-border-subtle, #2A2A2A)',
              borderRadius: 'var(--radius-sm, 4px)',
              color: 'var(--color-text-default, #D9D9D9)',
              cursor: 'pointer',
            }}
          >
            {t('drmFree.close')}
          </button>
        </div>
      </div>
    </div>
  );
};
