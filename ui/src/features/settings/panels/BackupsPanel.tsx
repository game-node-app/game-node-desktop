import React, { useEffect, useRef } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { IconAlertTriangle } from '@tabler/icons-react';
import {
  commands,
  coverAssetUrl,
  useAppInfo,
  useLocalGames,
} from '../../../api';
import { SelectField, Pill, Switch } from '../../../components/ui';
import { uiStore, useUiStore } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import { formatBytes } from '../../ports/catalog/format';
import {
  PathField,
  SettingsCard,
  SettingsPanel,
  SettingsRow,
} from '../components';
import {
  backupDraftChangedFields,
  backupDraftValue,
  updateBackupDraft,
} from '../settingsDraft';

const RETENTION_OPTIONS = [5, 10, 20, 50] as const;

export const BACKUP_USAGE_QUERY_KEY = ['local-backup-usage'] as const;

/** The Windows build owns local save backups; an unreported platform keeps today's behavior. */
function localFeaturesSupported(platform: string | null | undefined): boolean {
  return platform == null || platform === 'windows';
}

/**
 * Backups (S4, D12) on the group-save draft model: automatic protection,
 * destination and the default copies kept per game, all saved together from
 * the modal's unsaved bar. Uso do espaço reads the total and per-game space on
 * a worker and refreshes after backup, restore and prune events.
 */
export const BackupsPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const localGames = useLocalGames();
  const { data: appInfo } = useAppInfo();
  const queryClient = useQueryClient();
  const draft = useUiStore((state) => state.settingsDraft);
  const draftError = useUiStore((state) => state.settingsDraftError);

  const supported = localFeaturesSupported(appInfo?.platform);
  const baseline = localGames.settings;
  const changed = backupDraftChangedFields(draft, baseline);
  const automatic = backupDraftValue(draft, baseline, 'automatic');
  const destination = backupDraftValue(draft, baseline, 'destination');
  const retention = backupDraftValue(draft, baseline, 'retention');

  const usage = useQuery({
    queryKey: BACKUP_USAGE_QUERY_KEY,
    queryFn: commands.localBackupUsage,
    // The panel can be closed while backups run; a remount always re-reads.
    staleTime: 0,
    enabled: supported,
  });

  // Backup jobs, restore and pruning all publish snapshot changes; when the
  // backup state moves, the cached usage no longer describes the disk.
  const activity = [
    localGames.jobs
      .map((job) => `${job.id}:${job.state}:${job.attempts}:${job.backupId ?? ''}`)
      .join(','),
    localGames.versions.length,
    localGames.historyCount,
    localGames.historyBytes,
    localGames.restore ? 'restore' : 'idle',
  ].join('|');
  const activityRef = useRef(activity);
  useEffect(() => {
    if (activityRef.current === activity) return;
    activityRef.current = activity;
    void queryClient.invalidateQueries({ queryKey: BACKUP_USAGE_QUERY_KEY });
  }, [activity, queryClient]);

  const updateDraft = (patch: Parameters<typeof updateBackupDraft>[1]) => {
    uiStore.setState((previous) => ({
      ...previous,
      settingsDraft: updateBackupDraft(previous.settingsDraft, patch),
      settingsDraftError: null,
    }));
  };

  const handleBrowseDestination = async () => {
    try {
      const picked = await commands.pickSaveDestination();
      if (picked) {
        updateDraft({ destination: picked });
      }
    } catch {
      uiStore.setState((previous) => ({
        ...previous,
        settingsDraftError: t('settings.backups.chooseFolderFailed'),
      }));
    }
  };

  if (!supported) {
    return (
      <SettingsPanel
        title={t('settings.nav.backups')}
        description={t('settings.backups.description')}
      >
        <div className="flex items-center gap-xs">
          <Pill tone="neutral">{t('settings.unavailableOnThisSystem')}</Pill>
        </div>
        <p className="text-body leading-note text-text-secondary">
          {t('settings.backups.unsupportedHelp')}
        </p>
      </SettingsPanel>
    );
  }

  const data = usage.data;
  const totalBytes = data?.totalBytes ?? 0;
  const totalVersions = data?.totalVersions ?? 0;

  return (
    <SettingsPanel
      title={t('settings.nav.backups')}
      description={t('settings.backups.description')}
    >
      <SettingsCard title={t('settings.backups.automaticProtection')}>
        <SettingsRow
          label={t('settings.backups.protectAutomatically')}
          help={t('settings.backups.protectAutomaticallyHelp')}
          changed={changed.includes('automatic')}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              checked={automatic}
              onChange={(event) =>
                updateDraft({ automatic: event.currentTarget.checked })
              }
            />
          )}
        />
        <SettingsRow
          label={t('settings.backups.destination')}
          changed={changed.includes('destination')}
          previous={baseline.destination}
          control={(ids) => (
            <PathField
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              value={destination}
              changed={changed.includes('destination')}
              error={draftError ?? undefined}
              onChange={(value) => updateDraft({ destination: value })}
              onBrowse={() => void handleBrowseDestination()}
            />
          )}
        />
        <SettingsRow
          label={t('settings.backups.copiesKept')}
          help={t('settings.backups.copiesKeptHelp')}
          changed={changed.includes('retention')}
          control={(ids) => (
            <SelectField
              size="compact"
              className="h-[32px] w-[140px]"
              data-testid="settings-backups-copies"
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              value={String(retention)}
              options={RETENTION_OPTIONS.map((count) => ({
                value: String(count),
                label: t('settings.backups.copies', { n: count }),
              }))}
              onChange={(value) =>
                updateDraft({ retention: Number(value) })
              }
            />
          )}
        />
        <p className="border-t border-border-subtle px-md py-sm text-xs leading-note text-text-dimmed">
          {t('settings.backups.localNote')}{' '}
          {t('settings.perGameControlsAreInEachGame')}
        </p>
      </SettingsCard>

      <SettingsCard title={t('settings.backups.spaceUsed')}>
        {usage.isError ? (
          <div
            role="alert"
            className="flex items-start gap-compact border-t border-border-subtle px-md py-sm text-xs leading-note text-text-secondary"
          >
            <IconAlertTriangle
              size={14}
              stroke={2}
              aria-hidden="true"
              className="mt-[1px] shrink-0 text-warning"
            />
            <span>{t('settings.backups.usageError')}</span>
          </div>
        ) : !data ? (
          <p
            role="status"
            className="border-t border-border-subtle px-md py-sm text-xs text-text-dimmed"
          >
            {t('settings.backups.loading')}
          </p>
        ) : data.games.length === 0 ? (
          <p className="border-t border-border-subtle px-md py-sm text-xs text-text-dimmed">
            {t('settings.backups.empty')}
          </p>
        ) : (
          <>
            <p className="border-t border-border-subtle px-md py-sm text-sm font-semibold text-text-primary">
              {t('settings.backups.usageLine', {
                size: formatBytes(totalBytes, locale),
                n: totalVersions,
              })}
            </p>
            {data.games.map((game) => {
              const src = coverAssetUrl(game.coverFile);
              return (
                <div
                  key={game.entryKey}
                  className="flex items-center gap-xs border-t border-border-subtle px-md py-sm"
                >
                  <span className="flex h-[24px] w-[18px] shrink-0 items-center justify-center overflow-hidden rounded-[2px] bg-raised">
                    {src ? (
                      <img
                        src={src}
                        alt=""
                        loading="lazy"
                        decoding="async"
                        className="h-full w-full object-cover"
                      />
                    ) : null}
                  </span>
                  <span className="min-w-0 flex-1 truncate text-body text-text-default">
                    {game.title}
                  </span>
                  <span className="shrink-0 text-xs text-text-dimmed">
                    {t('settings.backups.usageLine', {
                      size: formatBytes(game.bytes, locale),
                      n: game.versions,
                    })}
                  </span>
                </div>
              );
            })}
          </>
        )}
      </SettingsCard>
    </SettingsPanel>
  );
};
