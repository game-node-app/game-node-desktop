import React, { useEffect, useState } from 'react';
import {
  IconAlertTriangle,
  IconArrowBackUp,
  IconFileText,
  IconFolder,
  IconLoader2,
  IconPlus,
  IconRefresh,
  IconX,
} from '@tabler/icons-react';
import {
  commands,
  useAppInfo,
  useLocalGames,
  useSnapshot,
  type AppPreferences,
  type WatchView,
} from '../../../api';
import { Button, Pill, Switch } from '../../../components/ui';
import { formatClock, formatRelativeDate, type Translator } from '../../library/detail/format';
import { useI18n } from '../../../i18n';
import {
  SettingsCard,
  SettingsPanel,
  SettingsRow,
} from '../components';

type DiscoveryPreference = 'discoveryOnStart' | 'discoveryRealtime';

/**
 * The Windows build owns local discovery and backups. An older host that does
 * not report a platform keeps the pre-redesign behavior.
 */
function localFeaturesSupported(platform: string | null | undefined): boolean {
  return platform == null || platform === 'windows';
}

/** "since …" wording for an unavailable folder, in the active locale. */
function unavailableSince(
  at: number | null,
  now: number,
  locale: string,
  t: Translator,
): string {
  if (at == null) return t('settings.drmFree.state.unavailable');
  const elapsed = Math.max(0, now - at);
  if (elapsed < 86_400) return formatClock(at, locale) ?? '';
  const relative = formatRelativeDate(at, now, locale, t);
  return relative.length === 0 ? relative : relative.charAt(0).toLowerCase() + relative.slice(1);
}

/**
 * Jogos DRM-free (S3, D11): watched folders with game count, last check and
 * availability, per-folder check, picker-based add, discovery preferences,
 * ignored discoveries with reset, and add-from-executable. Folder work runs
 * through the host's existing discovery commands; this panel never walks a
 * directory.
 */
export const DrmFreePanel: React.FC = () => {
  const { t, locale } = useI18n();
  const localGames = useLocalGames();
  const { data: appInfo } = useAppInfo();
  const { data: preferences } = useSnapshot((state) => state.preferences);

  const [overrides, setOverrides] = useState<
    Partial<Pick<AppPreferences, DiscoveryPreference>>
  >({});
  const [error, setError] = useState<string | null>(null);
  const [checkingAll, setCheckingAll] = useState(false);

  const supported = localFeaturesSupported(appInfo?.platform);
  const now = Math.floor(Date.now() / 1000);
  const watches = localGames.watches.filter((watch) => watch.enabled);
  const ignoredCount = localGames.candidates.filter(
    (candidate) => candidate.ignored,
  ).length;
  const busy = localGames.busy || checkingAll;
  const scanning = localGames.discoveryScanning;

  useEffect(() => {
    setOverrides((previous) => {
      const entries = Object.entries(previous);
      if (entries.length === 0) return previous;
      const next = { ...previous };
      let changed = false;
      for (const [key, value] of entries) {
        if (preferences[key as DiscoveryPreference] === value) {
          delete next[key as DiscoveryPreference];
          changed = true;
        }
      }
      return changed ? next : previous;
    });
  }, [preferences]);

  if (!supported) {
    return (
      <SettingsPanel
        title={t('settings.nav.drmFree')}
        description={t('settings.drmFree.description')}
      >
        <div className="flex items-center gap-xs">
          <Pill tone="neutral">{t('settings.unavailableOnThisSystem')}</Pill>
        </div>
        <p className="text-body leading-note text-text-secondary">
          {t('settings.drmFree.unsupportedHelp')}
        </p>
      </SettingsPanel>
    );
  }

  const preferenceValue = (key: DiscoveryPreference): boolean =>
    overrides[key] ?? preferences[key];

  const applyPreference = async (key: DiscoveryPreference, value: boolean) => {
    setOverrides((previous) => ({ ...previous, [key]: value }));
    setError(null);
    try {
      await commands.setAppPreference(
        key === 'discoveryOnStart'
          ? { discoveryOnStart: value }
          : { discoveryRealtime: value },
      );
    } catch {
      setOverrides((previous) => {
        const next = { ...previous };
        delete next[key];
        return next;
      });
      setError(t('settings.general.saveFailed'));
    }
  };

  const handleCheckAll = async () => {
    setCheckingAll(true);
    setError(null);
    try {
      await commands.rescanLocalFolders();
    } catch {
      setError(t('settings.drmFree.checkFailed'));
    } finally {
      setCheckingAll(false);
    }
  };

  const handleCheck = async (watch: WatchView) => {
    setError(null);
    try {
      await commands.checkLocalWatch(watch.id);
    } catch {
      setError(t('settings.drmFree.checkFailed'));
    }
  };

  const handleAddFolder = async () => {
    setError(null);
    try {
      await commands.pickLocalWatchFolder();
    } catch {
      setError(t('settings.drmFree.addFolderFailed'));
    }
  };

  return (
    <SettingsPanel
      title={t('settings.nav.drmFree')}
      description={t('settings.drmFree.description')}
      action={
        <Button
          size="sm"
          variant="secondary"
          leftIcon={IconRefresh}
          loading={scanning || checkingAll}
          disabled={busy || scanning}
          onClick={() => void handleCheckAll()}
        >
          {t('settings.drmFree.checkAll')}
        </Button>
      }
    >
      {error ? (
        <div
          role="alert"
          data-testid="settings-drm-free-error"
          className="flex items-start gap-compact rounded-md border border-danger bg-danger-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-danger"
          />
          <span>{error}</span>
        </div>
      ) : null}

      {localGames.discoveryError ? (
        <div
          role="alert"
          className="flex items-start gap-compact rounded-md border border-danger bg-danger-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-danger"
          />
          <span>{localGames.discoveryError}</span>
        </div>
      ) : null}

      {!localGames.discoveryError && localGames.discoveryIncomplete.length > 0 ? (
        <div
          role="status"
          className="flex items-start gap-compact rounded-md border border-warning bg-warning-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-warning"
          />
          <span>
            {t('DRM-free::Some folders could not be checked completely.')}
          </span>
        </div>
      ) : null}

      <SettingsCard
        title={t('settings.drmFree.watchedFolders')}
        action={
          <Button
            size="sm"
            variant="primary"
            leftIcon={IconPlus}
            disabled={busy}
            onClick={() => void handleAddFolder()}
          >
            {t('settings.drmFree.addFolder')}
          </Button>
        }
      >
        {watches.length === 0 ? (
          <p className="border-t border-border-subtle px-md py-sm text-xs text-text-dimmed">
            {t('settings.drmFree.empty')}
          </p>
        ) : (
          watches.map((watch) => {
            const unavailable = watch.state === 'unavailable';
            const checking = watch.state === 'checking';
            const meta = unavailable
              ? t('settings.drmFree.folderMetaUnavailable', {
                  n: watch.entryCount,
                  relative: unavailableSince(
                    watch.unavailableSince,
                    now,
                    locale,
                    t,
                  ),
                })
              : watch.lastCheckedAt === null
                ? t('settings.drmFree.folderMetaNever', {
                    n: watch.entryCount,
                  })
                : t('settings.drmFree.folderMeta', {
                    n: watch.entryCount,
                    time: formatClock(watch.lastCheckedAt, locale) ?? '',
                  });
            return (
              <SettingsRow
                key={watch.id}
                data-testid={`settings-watch-${watch.id}`}
                compact
                labelSize="compact"
                icon={unavailable ? IconAlertTriangle : IconFolder}
                iconTone={unavailable ? 'warning' : 'default'}
                label={watch.path}
                help={meta}
                aria-busy={checking || undefined}
                control={
                  <>
                    {checking ? (
                      <Pill tone="neutral">
                        <IconLoader2
                          size={12}
                          stroke={2}
                          aria-hidden="true"
                          className="animate-spin motion-reduce:animate-none"
                        />
                        {t('settings.drmFree.state.checking')}
                      </Pill>
                    ) : unavailable ? (
                      <Pill tone="warning">
                        {t('settings.drmFree.state.unavailable')}
                      </Pill>
                    ) : (
                      <Pill tone="success">
                        {t('settings.drmFree.state.watching')}
                      </Pill>
                    )}
                    {!unavailable && !checking ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        leftIcon={IconRefresh}
                        disabled={busy || scanning}
                        onClick={() => void handleCheck(watch)}
                      >
                        {t('settings.drmFree.check')}
                      </Button>
                    ) : null}
                    <Button
                      size="sm"
                      variant="ghost"
                      leftIcon={IconX}
                      onClick={() => void commands.removeLocalWatch(watch.id)}
                    >
                      {t('settings.drmFree.stopWatching')}
                    </Button>
                  </>
                }
              />
            );
          })
        )}
      </SettingsCard>

      <SettingsCard title={t('settings.drmFree.discovery')}>
        <SettingsRow
          label={t('settings.drmFree.discovery.onStart')}
          help={t('settings.drmFree.discovery.onStartHelp')}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              checked={preferenceValue('discoveryOnStart')}
              onChange={(event) =>
                void applyPreference(
                  'discoveryOnStart',
                  event.currentTarget.checked,
                )
              }
            />
          )}
        />
        <SettingsRow
          label={t('settings.drmFree.discovery.realtime')}
          help={t('settings.drmFree.discovery.realtimeHelp')}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              checked={preferenceValue('discoveryRealtime')}
              onChange={(event) =>
                void applyPreference(
                  'discoveryRealtime',
                  event.currentTarget.checked,
                )
              }
            />
          )}
        />
        <SettingsRow
          label={t('settings.drmFree.ignored')}
          help={
            ignoredCount === 1
              ? t('settings.drmFree.ignored.one', { n: ignoredCount })
              : t('settings.drmFree.ignored.other', { n: ignoredCount })
          }
          control={
            <Button
              size="sm"
              variant="secondary"
              leftIcon={IconArrowBackUp}
              disabled={ignoredCount === 0 || busy}
              onClick={() => void commands.resetLocalIgnores()}
            >
              {t('settings.drmFree.resetIgnored')}
            </Button>
          }
        />
      </SettingsCard>

      <SettingsCard title={t('settings.drmFree.addManually')}>
        <SettingsRow
          icon={IconPlus}
          label={t('settings.drmFree.addFromExecutable')}
          help={t('settings.drmFree.addFromExecutableHelp')}
          control={
            <Button
              size="sm"
              variant="secondary"
              leftIcon={IconFileText}
              disabled={busy}
              onClick={() => void commands.pickLocalExecutable()}
            >
              {t('settings.drmFree.chooseExecutable')}
            </Button>
          }
        />
      </SettingsCard>
    </SettingsPanel>
  );
};
