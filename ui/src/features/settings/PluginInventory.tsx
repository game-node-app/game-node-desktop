import React from 'react';
import { useQuery } from '@tanstack/react-query';
import { commands, type PluginInfo } from '../../api';
import { useI18n } from '../../i18n';
import {
  settingsCard,
  settingsPill,
  settingsPillSuccess,
  settingsPillWarning,
  settingsRow,
  settingsRowDescription,
  settingsRowInfo,
  settingsRowTitle,
} from './settingsClasses';

const inventoryQueryKey = ['plugin-inventory'] as const;

function statusLabel(status: PluginInfo['status'], t: (key: string) => string): string {
  return t(`plugins.status.${status}`);
}

export const PluginInventory: React.FC = () => {
  const { t } = useI18n();
  const { data = [], isPending, isError } = useQuery({
    queryKey: inventoryQueryKey,
    queryFn: commands.pluginInventory,
    staleTime: 30_000,
  });

  return (
    <section aria-label={t('plugins.inventoryTitle')}>
      <h3 className="mb-1 text-sm font-semibold text-white">{t('plugins.inventoryTitle')}</h3>
      <p className={`${settingsRowDescription} mb-compact`}>{t('plugins.inventoryDescription')}</p>
      <div className={settingsCard}>
        {isPending ? (
          <p className={settingsRowDescription} role="status">{t('plugins.loading')}</p>
        ) : isError ? (
          <p className={settingsRowDescription} role="alert">{t('plugins.loadFailed')}</p>
        ) : data.length === 0 ? (
          <p className={settingsRowDescription}>{t('plugins.empty')}</p>
        ) : data.map((plugin) => {
          const stateClass = plugin.status === 'installed'
            ? settingsPillSuccess
            : plugin.status === 'missing' || plugin.status === 'failed' || plugin.status === 'incompatible'
              ? settingsPillWarning
              : settingsPill;
          return (
            <article key={plugin.pluginId} className={`${settingsRow} items-start py-1`}>
              <div className={`${settingsRowInfo} gap-1`}>
                <div className="flex flex-wrap items-center gap-compact">
                  <span className={settingsRowTitle}>{plugin.name}</span>
                  <span className={settingsPill}>{plugin.source === 'official' ? t('plugins.official') : t('plugins.community')}</span>
                  <span className={stateClass}>{statusLabel(plugin.status, t)}</span>
                </div>
                <p className={settingsRowDescription}>{t('plugins.session')}: {plugin.session === 'active' ? t('plugins.sessionActive') : t('plugins.sessionStopped')}</p>
                <div>
                  <p className={settingsRowDescription}>{t('plugins.grants')}</p>
                  <ul className="mt-1 flex flex-wrap gap-1" aria-label={t('plugins.grants')}>
                    {plugin.capabilities.map((grant) => (
                      <li key={grant} className={settingsPill}>{grant}</li>
                    ))}
                  </ul>
                </div>
                <div>
                  <p className={settingsRowDescription}>{t('plugins.nativeAccess')}</p>
                  <ul className="mt-1 flex flex-wrap gap-1" aria-label={t('plugins.nativeAccess')}>
                    {plugin.nativeAccess.map((access) => (
                      <li key={access} className={settingsPill}>{t(`plugins.native.${access}`)}</li>
                    ))}
                  </ul>
                  <p className={`${settingsRowDescription} mt-1`}>{t('plugins.nativeAccessNote')}</p>
                </div>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
};
