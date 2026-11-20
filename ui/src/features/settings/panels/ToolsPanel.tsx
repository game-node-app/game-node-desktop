import React, { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Menu } from '@mantine/core';
import {
  IconAlertTriangle,
  IconBolt,
  IconCircleCheck,
  IconDeviceGamepad2,
  IconExternalLink,
  IconPackage,
  IconTool,
  IconTrash,
} from '@tabler/icons-react';
import {
  commands,
  detectPortPlatform,
  PORTS_TOOLS_CACHE_QUERY_KEY,
  portToolsTotalBytes,
  useAppInfo,
  useLibraryView,
  useSnapshot,
  useTools,
  type AppPreferencePatch,
  type AppPreferences,
  type ToolCacheEntryView,
  type ToolCatalogItemView,
  type ToolChannel,
} from '../../../api';
import {
  Button,
  MenuSurface,
  Pill,
  SegmentedControl,
  Switch,
} from '../../../components/ui';
import { pushToast, setActiveDestination, closeSettingsModal } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import { formatBytes } from '../../ports/catalog/format';
import {
  IconBox,
  SettingsCard,
  SettingsPanel,
  SettingsRow,
} from '../components';

interface InstalledGame {
  key: string;
  title: string;
}

/** The games whose installations reference `toolId`. */
function gamesWithTool(
  toolId: string,
  installedByEntry: Record<string, { toolId: string }[]>,
  titles: Map<string, string>,
): InstalledGame[] {
  return Object.entries(installedByEntry)
    .filter(([, tools]) => tools.some((tool) => tool.toolId === toolId))
    .map(([key]) => ({ key, title: titles.get(key) ?? key }));
}

/**
 * Ferramentas (S5): the supported-tool cards with licence and source, the
 * update-check and default-channel preferences, the per-version download
 * cache, and the Windows-only PC-ports helper tools and cache. Preferences
 * apply immediately with an optimistic value and roll back on a command error
 * (D13, D4).
 */
export const ToolsPanel: React.FC = () => {
  const { t, locale } = useI18n();
  const tools = useTools();
  const library = useLibraryView();
  const { data: appInfo } = useAppInfo();
  const { data: preferences } = useSnapshot((snapshot) => snapshot.preferences);
  const queryClient = useQueryClient();

  // PC ports are Windows-only in this release, like the sidebar destination.
  const platform = detectPortPlatform(
    typeof navigator !== 'undefined' ? navigator.userAgent : null,
    appInfo?.platform,
  );
  const showPorts = platform === 'windows';

  const portsCacheQuery = useQuery({
    queryKey: PORTS_TOOLS_CACHE_QUERY_KEY,
    queryFn: () => commands.portsToolsCache(),
    enabled: showPorts,
    staleTime: 0,
  });
  const portsCache = portsCacheQuery.data;
  const helperTools = portsCache?.helperTools ?? [];
  const portsDownloadsBytes = portsCache?.downloadsBytes ?? 0;
  const portsTotalBytes = portToolsTotalBytes(portsCache);

  const [clearingCache, setClearingCache] = useState(false);
  const [clearingPortsCache, setClearingPortsCache] = useState(false);
  const [overrides, setOverrides] = useState<Partial<AppPreferences>>({});
  const [error, setError] = useState<string | null>(null);

  const catalog = tools?.catalog ?? [];
  const cacheEntries = tools?.cacheEntries ?? [];
  const totalCacheArchives = tools?.totalCacheArchives ?? 0;
  const totalCacheBytes = tools?.totalCacheBytes ?? 0;

  const rowTitles = new Map<string, string>([
    ...(library?.paletteIndex ?? []).map(
      (entry): [string, string] => [entry.key, entry.title],
    ),
    ...(library?.rows ?? []).map(
      (row): [string, string] => [row.key, row.title],
    ),
  ]);

  // Drop an optimistic value as soon as the published snapshot confirms it.
  useEffect(() => {
    setOverrides((previous) => {
      const entries = Object.entries(previous);
      if (entries.length === 0) return previous;
      const next: Partial<AppPreferences> = { ...previous };
      let changed = false;
      for (const [key, value] of entries) {
        const typedKey = key as keyof AppPreferences;
        if (preferences[typedKey] === value) {
          delete next[typedKey];
          changed = true;
        }
      }
      return changed ? next : previous;
    });
  }, [preferences]);

  const preferenceValue = <K extends keyof AppPreferences>(
    key: K,
  ): AppPreferences[K] =>
    Object.prototype.hasOwnProperty.call(overrides, key)
      ? (overrides[key] as AppPreferences[K])
      : preferences[key];

  const applyPreference = async <K extends keyof AppPreferences>(
    key: K,
    value: AppPreferences[K],
    patch: AppPreferencePatch,
  ) => {
    setOverrides((previous) => ({ ...previous, [key]: value }));
    setError(null);
    try {
      await commands.setAppPreference(patch);
    } catch {
      setOverrides((previous) => {
        const next = { ...previous };
        delete next[key];
        return next;
      });
      setError(t('settings.tools.saveFailed'));
    }
  };

  const handleClearCache = async () => {
    if (clearingCache) return;
    setClearingCache(true);
    try {
      await commands.clearToolCache();
      pushToast('success', t('settings.toolCacheCleared'));
    } catch {
      pushToast('danger', t('settings.tools.clearFailed'));
    } finally {
      setClearingCache(false);
    }
  };

  const handleClearPortsCache = async () => {
    if (clearingPortsCache) return;
    setClearingPortsCache(true);
    try {
      const next = await commands.portsClearCache();
      queryClient.setQueryData(PORTS_TOOLS_CACHE_QUERY_KEY, next);
      pushToast('success', t('settings.portCacheCleared'));
    } catch {
      // The host refuses while a queued, running or executable-awaiting install owns the cache.
      pushToast(
        'danger',
        t('settings.aPortInstallIsRunningClearTheCache'),
      );
    } finally {
      setClearingPortsCache(false);
    }
  };

  const handleOpenSource = (url: string) => {
    void commands.openToolSource(url).catch(() => undefined);
  };

  const openToolGames = (game: InstalledGame) => {
    closeSettingsModal();
    setActiveDestination('library');
    void commands.libraryInput({ select: { key: game.key, mode: 'replace' } });
  };

  const cacheSummary =
    totalCacheArchives === 1
      ? t('settings.tools.cacheSummary.one', {
          size: formatBytes(totalCacheBytes, locale),
          n: totalCacheArchives,
        })
      : t('settings.tools.cacheSummary.other', {
          size: formatBytes(totalCacheBytes, locale),
          n: totalCacheArchives,
        });

  return (
    <SettingsPanel
      title={t('settings.nav.tools')}
      description={t('settings.tools.description')}
    >
      {error ? (
        <div
          role="alert"
          data-testid="settings-tools-error"
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

      <SettingsCard title={t('settings.tools.supportedTools')}>
        {catalog.map((tool: ToolCatalogItemView) => {
          const games = gamesWithTool(
            tool.id,
            tools.installedByEntry,
            rowTitles,
          );
          const ToolIcon = tool.id === 'optiscaler' ? IconBolt : IconTool;
          return (
            <div
              key={tool.id}
              data-testid={`settings-tool-${tool.id}`}
              className="flex flex-wrap items-center gap-row border-t border-border-subtle px-md py-row"
            >
              <IconBox icon={ToolIcon} tone="accent" />
              <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <span className="text-title font-medium text-text-primary">
                  {tool.name}
                </span>
                <p className="max-w-settings-copy text-xs leading-note text-text-dimmed">
                  {tool.summary}
                </p>
                {tool.sourceUrl ? (
                  <button
                    type="button"
                    className="w-fit rounded-sm text-xs text-text-dimmed underline-offset-[3px] transition-colors hover:text-text-default hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                    onClick={() => handleOpenSource(tool.sourceUrl)}
                  >
                    {t('settings.tools.licenseSource', {
                      license: tool.license,
                    })}
                  </button>
                ) : (
                  <span className="text-xs text-text-dimmed">
                    {t('settings.tools.licenseSource', {
                      license: tool.license,
                    })}
                  </span>
                )}
              </div>

              {games.length > 0 ? (
                <>
                  <Pill tone="success" icon={IconCircleCheck}>
                    {games.length === 1
                      ? t('settings.tools.inGames.one', { n: games.length })
                      : t('settings.tools.inGames.other', { n: games.length })}
                  </Pill>
                  <MenuSurface position="bottom-end" width={240} withinPortal>
                    <Menu.Target>
                      <Button size="sm" variant="secondary">
                        {t('settings.tools.viewGames')}
                      </Button>
                    </Menu.Target>
                    <Menu.Dropdown>
                      {games.map((game) => (
                        <Menu.Item
                          key={game.key}
                          leftSection={
                            <IconDeviceGamepad2
                              size={16}
                              stroke={2}
                              aria-hidden="true"
                            />
                          }
                          onClick={() => openToolGames(game)}
                        >
                          {game.title}
                        </Menu.Item>
                      ))}
                    </Menu.Dropdown>
                  </MenuSurface>
                </>
              ) : (
                <Pill tone="neutral">{t('settings.tools.notInstalled')}</Pill>
              )}
            </div>
          );
        })}

        <SettingsRow
          label={t('settings.tools.autoCheck')}
          help={t('settings.tools.autoCheckHelp')}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              checked={preferenceValue('toolAutoCheck')}
              onChange={(event) =>
                void applyPreference(
                  'toolAutoCheck',
                  event.currentTarget.checked,
                  { toolAutoCheck: event.currentTarget.checked },
                )
              }
            />
          )}
        />
        <SettingsRow
          label={t('settings.tools.defaultChannel')}
          help={t('settings.tools.defaultChannelHelp')}
          control={(ids) => (
            <SegmentedControl<ToolChannel>
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              segmentPadding="regular"
              value={preferenceValue('toolDefaultChannel')}
              onChange={(next) => {
                if (next === preferenceValue('toolDefaultChannel')) return;
                void applyPreference('toolDefaultChannel', next, {
                  toolDefaultChannel: next,
                });
              }}
              data={[
                {
                  value: 'stable',
                  label: t('settings.tools.channel.stable'),
                },
                {
                  value: 'nightly',
                  label: t('settings.tools.channel.nightly'),
                },
              ]}
            />
          )}
        />
      </SettingsCard>

      <SettingsCard
        title={t('settings.tools.downloadCache')}
        action={
          <Button
            size="sm"
            variant="secondary"
            leftIcon={IconTrash}
            loading={clearingCache}
            disabled={totalCacheArchives === 0}
            onClick={() => void handleClearCache()}
          >
            {t('settings.tools.clearCache')}
          </Button>
        }
      >
        <div className="flex flex-col gap-compact border-t border-border-subtle px-md pt-row pb-row">
          <div className="flex flex-wrap items-center gap-xs">
            <span className="text-sm font-semibold text-text-primary">
              {cacheSummary}
            </span>
            <span className="ml-auto text-xs text-text-dimmed">
              {t('settings.tools.clearCacheHelp')}
            </span>
          </div>

          {cacheEntries.length > 0 ? (
            <ul
              aria-label={t('settings.tools.downloadCache')}
              className="flex flex-col"
            >
              {cacheEntries.map((entry: ToolCacheEntryView) => {
                const toolName =
                  catalog.find((tool) => tool.id === entry.toolId)?.name ??
                  entry.toolId;
                return (
                  <li
                    key={`${entry.toolId}-${entry.channel}-${entry.version}`}
                    className="flex flex-wrap items-center gap-compact border-t border-border-subtle py-sm"
                  >
                    <IconPackage
                      size={16}
                      stroke={2}
                      aria-hidden="true"
                      className="shrink-0 text-text-secondary"
                    />
                    <span className="min-w-0 text-body text-text-default">
                      {toolName} {entry.version}
                    </span>
                    {entry.inUse ? (
                      <Pill tone="success">{t('settings.tools.inUse')}</Pill>
                    ) : null}
                    {entry.channel === 'nightly' ? (
                      <Pill tone="warning">
                        {t('settings.tools.channel.nightly')}
                      </Pill>
                    ) : null}
                    <span className="ml-auto text-xs text-text-dimmed">
                      {formatBytes(entry.bytes, locale)}
                      {entry.sha256Verified
                        ? ` · ${t('settings.tools.shaVerified')}`
                        : ''}
                    </span>
                  </li>
                );
              })}
            </ul>
          ) : null}
        </div>
      </SettingsCard>

      {showPorts ? (
        <>
          <SettingsCard
            title={t('settings.tools.helperTools')}
            data-testid="ports-helper-tools"
          >
            {helperTools.length === 0 ? (
              <div className="flex flex-col gap-[3px] border-t border-border-subtle px-md py-row">
                <span className="text-sm font-medium text-text-primary">
                  {t('settings.noExtractionToolConfigured')}
                </span>
                <span className="max-w-settings-copy text-xs leading-note text-text-dimmed">
                  {t('settings.pcPortsShowsOneHereOnceTheOverlay')}
                </span>
              </div>
            ) : (
              helperTools.map((tool) => (
                <div
                  key={tool.id}
                  className="flex flex-wrap items-center gap-row border-t border-border-subtle px-md py-sm"
                >
                  <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                    <span className="text-sm font-medium text-text-primary">
                      {tool.name}
                    </span>
                    <span className="text-xs text-text-dimmed">
                      {t('settings.pinnedVersion', { version: tool.version })}
                    </span>
                  </div>
                  <Pill tone="neutral">{tool.license}</Pill>
                  <Pill tone="neutral">
                    {formatBytes(tool.cacheBytes, locale)}
                  </Pill>
                  {tool.sourceUrl ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      leftIcon={IconExternalLink}
                      onClick={() => handleOpenSource(tool.sourceUrl as string)}
                    >
                      {t('Source')}
                    </Button>
                  ) : null}
                </div>
              ))
            )}
          </SettingsCard>

          <SettingsCard
            title={t('settings.tools.portsCache')}
            action={
              <Button
                size="sm"
                variant="secondary"
                leftIcon={IconTrash}
                data-testid="ports-clear-cache"
                loading={clearingPortsCache}
                disabled={portsTotalBytes === 0 || clearingPortsCache}
                onClick={() => void handleClearPortsCache()}
              >
                {t('settings.tools.clearCache')}
              </Button>
            }
          >
            <div className="flex flex-wrap items-center gap-row border-t border-border-subtle px-md py-row">
              <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
                <span className="text-sm font-medium text-text-primary">
                  {t('settings.cachedDownloads')}
                </span>
                <span className="text-xs text-text-dimmed" data-testid="ports-downloads-size">
                  {formatBytes(portsDownloadsBytes, locale)}
                </span>
              </div>
              {portsCache?.installActive ? (
                <Pill tone="warning">{t('settings.installRunning')}</Pill>
              ) : null}
            </div>
          </SettingsCard>
        </>
      ) : null}
    </SettingsPanel>
  );
};
