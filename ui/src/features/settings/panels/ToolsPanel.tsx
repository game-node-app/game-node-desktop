import React from 'react';
import { commands, useTools } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import {
  settingsButtonGhost,
  settingsCard,
  settingsPill,
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

  const cacheArchives = tools?.totalCacheArchives ?? 0;
  const cacheBytes = tools?.totalCacheBytes ?? 0;
  const catalog = tools?.catalog ?? [];

  const handleClearCache = () => {
    commands.clearToolCache();
    pushToast('success', t('Tool cache cleared'));
  };

  const handleOpenSource = (url: string) => {
    commands.openToolSource(url);
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
    </div>
  );
};
