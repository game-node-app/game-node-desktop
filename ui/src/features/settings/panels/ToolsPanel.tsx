import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  commands,
  detectPortPlatform,
  PORTS_TOOLS_CACHE_QUERY_KEY,
  portToolsTotalBytes,
  useAppInfo,
  useTools,
} from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import {
  settingsButtonGhost,
  settingsCard,
  settingsPill,
  settingsPillWarning,
  settingsRow,
  settingsRowDescription,
  settingsRowInfo,
  settingsRowTitle,
  settingsSection,
  settingsSectionDescription,
  settingsSectionTitle,
} from '../settingsClasses';

function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const val = bytes / Math.pow(1024, i);
  return `${val.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export const ToolsPanel: React.FC = () => {
  const { t } = useI18n();
  const tools = useTools();
  const { data: appInfo } = useAppInfo();
  const queryClient = useQueryClient();
  const [clearingPortsCache, setClearingPortsCache] = useState(false);

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

  const cacheArchives = tools?.totalCacheArchives ?? 0;
  const cacheBytes = tools?.totalCacheBytes ?? 0;
  const catalog = tools?.catalog ?? [];

  const handleClearCache = () => {
    void commands.clearToolCache();
    pushToast('success', t('Tool cache cleared'));
  };

  const handleOpenSource = (url: string) => {
    commands.openToolSource(url);
  };

  const handleClearPortsCache = async () => {
    if (clearingPortsCache) return;
    setClearingPortsCache(true);
    try {
      const next = await commands.portsClearCache();
      queryClient.setQueryData(PORTS_TOOLS_CACHE_QUERY_KEY, next);
      pushToast('success', t('Port cache cleared'));
    } catch {
      // The host refuses while a queued, running or executable-awaiting install owns the cache.
      pushToast(
        'danger',
        t('A port install is running. Clear the cache when it finishes.'),
      );
    } finally {
      setClearingPortsCache(false);
    }
  };

  const cacheDesc =
    cacheArchives > 0
      ? `${cacheArchives} ${t('archives cached') || 'archives cached'} (${formatBytes(cacheBytes)})`
      : t('No tool archives cached') || 'No tool archives cached';

  return (
    <div className={settingsSection}>
      {/* Tool Download Cache */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Tool download cache') || 'Tool download cache'}</h3>
        <span className={settingsSectionDescription}>
          {t('Archives downloaded from GitHub for offline install and reuse across games.') ||
            'Archives downloaded from GitHub for offline install and reuse across games.'}
        </span>

        <div className={settingsCard}>
          <div className={settingsRow}>
            <div className={settingsRowInfo}>
              <span className={settingsRowTitle}>{t('Cached archives') || 'Cached archives'}</span>
              <span className={settingsRowDescription}>{cacheDesc}</span>
            </div>

            <div className="flex items-center gap-compact">
              <span className={settingsPill}>{formatBytes(cacheBytes)}</span>
              <button
                type="button"
                className={settingsButtonGhost}
                disabled={cacheArchives === 0}
                onClick={handleClearCache}
              >
                {t('Clear cache') || 'Clear cache'}
              </button>
            </div>
          </div>
        </div>
      </div>

      {/* Supported Tools / Catalog */}
      <div className={settingsSection}>
        <h3 className={settingsSectionTitle}>{t('Supported tools') || 'Supported tools'}</h3>
        <span className={settingsSectionDescription}>
          {t('Built-in tool catalog and official open-source licenses.') ||
            'Built-in tool catalog and official open-source licenses.'}
        </span>

        <div className={settingsCard}>
          {catalog.length === 0 ? (
            <span className={settingsRowDescription}>OptiScaler</span>
          ) : (
            catalog.map((tool) => (
              <div key={tool.id} className={`${settingsRow} py-1.5`}>
                <div className={settingsRowInfo}>
                  <span className={settingsRowTitle}>{tool.name}</span>
                  <span className={settingsRowDescription}>{tool.summary}</span>
                </div>
                <div className="flex items-center gap-compact">
                  <span className={settingsPill}>{tool.license}</span>
                  <button
                    type="button"
                    className={settingsButtonGhost}
                    onClick={() => handleOpenSource(tool.sourceUrl)}
                  >
                    {t('Source') || 'Source'}
                  </button>
                </div>
              </div>
            ))
          )}
        </div>
      </div>

      {showPorts && (
        <>
          {/* Helper tools pinned by the PC ports overlay */}
          <div className={settingsSection}>
            <h3 className={settingsSectionTitle}>{t('Helper tools') || 'Helper tools'}</h3>
            <span className={settingsSectionDescription}>
              {t('Extraction tools pinned by the PC ports overlay.') ||
                'Extraction tools pinned by the PC ports overlay.'}
            </span>

            <div className={settingsCard} data-testid="ports-helper-tools">
              {helperTools.length === 0 ? (
                <div className={settingsRow}>
                  <div className={settingsRowInfo}>
                    <span className={settingsRowTitle}>
                      {t('No extraction tool configured') || 'No extraction tool configured'}
                    </span>
                    <span className={settingsRowDescription}>
                      {t('PC ports shows one here once the overlay pins an extraction tool.') ||
                        'PC ports shows one here once the overlay pins an extraction tool.'}
                    </span>
                  </div>
                </div>
              ) : (
                helperTools.map((tool) => (
                  <div key={tool.id} className={`${settingsRow} py-1.5`}>
                    <div className={settingsRowInfo}>
                      <span className={settingsRowTitle}>{tool.name}</span>
                      <span className={settingsRowDescription}>
                        {t('Pinned version {version}', { version: tool.version })}
                      </span>
                    </div>
                    <div className="flex items-center gap-compact">
                      <span className={settingsPill}>{tool.license}</span>
                      <span className={settingsPill}>{formatBytes(tool.cacheBytes)}</span>
                      {tool.sourceUrl && (
                        <button
                          type="button"
                          className={settingsButtonGhost}
                          onClick={() => handleOpenSource(tool.sourceUrl as string)}
                        >
                          {t('Source') || 'Source'}
                        </button>
                      )}
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>

          {/* Port release download cache */}
          <div className={settingsSection}>
            <h3 className={settingsSectionTitle}>
              {t('PC ports download cache') || 'PC ports download cache'}
            </h3>
            <span className={settingsSectionDescription}>
              {t(
                'Release assets and pinned extraction tools cached for PC port installs. Clearing it never removes installed ports.',
              ) ||
                'Release assets and pinned extraction tools cached for PC port installs. Clearing it never removes installed ports.'}
            </span>

            <div className={settingsCard}>
              <div className={settingsRow}>
                <div className={settingsRowInfo}>
                  <span className={settingsRowTitle}>
                    {t('Cached downloads') || 'Cached downloads'}
                  </span>
                  <span className={settingsRowDescription} data-testid="ports-downloads-size">
                    {formatBytes(portsDownloadsBytes)}
                  </span>
                </div>

                <div className="flex items-center gap-compact">
                  {portsCache?.installActive && (
                    <span className={settingsPillWarning}>
                      {t('Install running') || 'Install running'}
                    </span>
                  )}
                  <button
                    type="button"
                    data-testid="ports-clear-cache"
                    className={settingsButtonGhost}
                    disabled={portsTotalBytes === 0 || clearingPortsCache}
                    onClick={() => {
                      void handleClearPortsCache();
                    }}
                  >
                    {t('Clear cache') || 'Clear cache'}
                  </button>
                </div>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
};
