import React from 'react';
import { commands, useTools } from '../../../api';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';

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
    <div className="settings-section">
      {/* Tool Download Cache */}
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Tool download cache') || 'Tool download cache'}</h3>
        <span className="settings-section-description">
          {t('Archives downloaded from GitHub for offline install and reuse across games.') ||
            'Archives downloaded from GitHub for offline install and reuse across games.'}
        </span>

        <div className="settings-card">
          <div className="settings-row">
            <div className="settings-row-info">
              <span className="settings-row-title">{t('Cached archives') || 'Cached archives'}</span>
              <span className="settings-row-description">{cacheDesc}</span>
            </div>

            <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
              <span className="settings-pill">{formatBytes(cacheBytes)}</span>
              <button
                type="button"
                className="settings-button settings-button-ghost"
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
      <div className="settings-section">
        <h3 className="settings-section-title">{t('Supported tools') || 'Supported tools'}</h3>
        <span className="settings-section-description">
          {t('Built-in tool catalog and official open-source licenses.') ||
            'Built-in tool catalog and official open-source licenses.'}
        </span>

        <div className="settings-card">
          {catalog.length === 0 ? (
            <span className="settings-row-description">OptiScaler</span>
          ) : (
            catalog.map((tool) => (
              <div key={tool.id} className="settings-row" style={{ padding: '6px 0' }}>
                <div className="settings-row-info">
                  <span className="settings-row-title">{tool.name}</span>
                  <span className="settings-row-description">{tool.summary}</span>
                </div>
                <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                  <span className="settings-pill">{tool.license}</span>
                  <button
                    type="button"
                    className="settings-button settings-button-ghost"
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
