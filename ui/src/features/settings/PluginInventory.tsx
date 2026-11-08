import { providerLabel } from '../../i18n/providerLabel';
import React, { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { commands, type PluginInfo } from '../../api';
import { useI18n } from '../../i18n';
import {
  settingsCard,
  settingsPill,
  settingsPillSuccess,
  settingsPillWarning,
  settingsRowDescription,
  settingsRowTitle,
} from './settingsClasses';

const inventoryQueryKey = ['plugin-inventory'] as const;

function statusLabel(status: PluginInfo['status'], t: (key: string) => string): string {
  return t(`plugins.status.${status}`);
}

export const PluginInventory: React.FC = () => {
  const { t } = useI18n();
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const { data = [], isPending, isError } = useQuery({
    queryKey: inventoryQueryKey,
    queryFn: commands.pluginInventory,
    staleTime: 30_000,
  });
  const selectedPlugin = data.find((plugin) => plugin.pluginId === selectedPluginId) ?? data[0];

  const stateClass = (status: PluginInfo['status']) => status === 'installed'
    ? settingsPillSuccess
    : status === 'missing' || status === 'failed' || status === 'incompatible'
      ? settingsPillWarning
      : settingsPill;

  return (
    <section aria-label={t('plugins.inventoryTitle')}>
      <p className={`${settingsRowDescription} mb-compact`}>{t('plugins.inventoryDescription')}</p>
      {isPending ? (
        <div className={settingsCard}>
          <p className={settingsRowDescription} role="status">{t('plugins.loading')}</p>
        </div>
      ) : isError ? (
        <div className={settingsCard}>
          <p className={settingsRowDescription} role="alert">{t('plugins.loadFailed')}</p>
        </div>
      ) : data.length === 0 ? (
        <div className={settingsCard}>
          <p className={settingsRowDescription}>{t('plugins.empty')}</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-md lg:grid-cols-[minmax(180px,0.8fr)_minmax(0,1.6fr)]">
          <div className={settingsCard} role="group" aria-label={t('plugins.inventoryTitle')}>
            {data.map((plugin) => (
              <button
                key={plugin.pluginId}
                type="button"
                aria-pressed={selectedPlugin?.pluginId === plugin.pluginId}
                className={`flex w-full flex-col items-start gap-1 rounded-sm border px-sm py-compact text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand ${selectedPlugin?.pluginId === plugin.pluginId ? 'border-brand bg-accent-subtle' : 'border-border-subtle bg-raised hover:bg-[#2e2e2e]'}`}
                onClick={() => setSelectedPluginId(plugin.pluginId)}
              >
                <span className={settingsRowTitle}>{providerLabel(plugin.providerId, plugin.name, t)}</span>
                <span className="flex flex-wrap gap-1">
                  <span className={settingsPill}>{plugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
                  <span className={stateClass(plugin.status)}>{statusLabel(plugin.status, t)}</span>
                </span>
              </button>
            ))}
          </div>
          {selectedPlugin && (
            <article className={`${settingsCard} self-start`} aria-label={t('plugins.details')}>
              <div className="flex flex-wrap items-center gap-compact">
                <h3 className={settingsRowTitle}>{providerLabel(selectedPlugin.providerId, selectedPlugin.name, t)}</h3>
                <span className={settingsPill}>{selectedPlugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
                <span className={stateClass(selectedPlugin.status)}>{statusLabel(selectedPlugin.status, t)}</span>
              </div>
              <p className={settingsRowDescription}>{t('plugins.identifier')}: {selectedPlugin.pluginId}</p>
              <p className={settingsRowDescription}>{t('plugins.session')}: {selectedPlugin.session === 'active' ? t('plugins.sessionActive') : t('plugins.sessionStopped')}</p>
              <div>
                <p className={settingsRowDescription}>{t('plugins.grants')}</p>
                <ul className="mt-1 flex flex-wrap gap-1" aria-label={t('plugins.grants')}>
                  {selectedPlugin.capabilities.map((grant) => (
                    <li key={grant} className={settingsPill}>{grant}</li>
                  ))}
                </ul>
              </div>
              <div>
                <p className={settingsRowDescription}>{t('plugins.nativeAccess')}</p>
                <ul className="mt-1 flex flex-wrap gap-1" aria-label={t('plugins.nativeAccess')}>
                  {selectedPlugin.nativeAccess.map((access) => (
                    <li key={access} className={settingsPill}>{t(`plugins.native.${access}`)}</li>
                  ))}
                </ul>
                <p className={`${settingsRowDescription} mt-1`}>{t('plugins.nativeAccessNote')}</p>
              </div>
            </article>
          )}
        </div>
      )}
    </section>
  );
};
