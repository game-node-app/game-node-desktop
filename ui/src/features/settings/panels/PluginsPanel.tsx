import React from 'react';
import { useQuery } from '@tanstack/react-query';
import { IconPuzzle } from '@tabler/icons-react';
import { commands } from '../../../api/commands';
import { useI18n } from '../../../i18n';
import { settingsCardRoomy, settingsPill, settingsPillWarning } from '../settingsClasses';

export const PluginsPanel: React.FC = () => {
  const { t } = useI18n();
  const { data: plugins = [], isPending, isError } = useQuery({
    queryKey: ['plugins'],
    queryFn: commands.getPlugins,
    staleTime: 60_000,
  });

  return (
    <div className="flex flex-col gap-md">
      <div className="rounded-sm border border-border-subtle bg-secondary p-md text-sm text-[var(--settings-text-secondary)]">
        {t('settings.pluginsDescription')}
      </div>
      {isPending && <p className="text-sm text-[var(--settings-text-secondary)]">{t('settings.pluginsLoading')}</p>}
      {isError && <p role="alert" className="text-sm text-danger">{t('settings.pluginsLoadError')}</p>}
      {!isPending && !isError && plugins.length === 0 && (
        <p className="text-sm text-[var(--settings-text-secondary)]">{t('settings.pluginsEmpty')}</p>
      )}
      {plugins.map((plugin) => (
        <article key={plugin.pluginId} className={settingsCardRoomy}>
          <div className="flex items-start justify-between gap-md">
            <div className="flex min-w-0 items-start gap-sm">
              <IconPuzzle className="mt-0.5 shrink-0 text-brand" size={20} aria-hidden="true" />
              <div className="min-w-0">
                <h3 className="text-sm font-semibold text-white">
                  {plugin.providerId === 'packagekit' ? t('settings.linuxPackageManager') : plugin.name}
                </h3>
                <p className="mt-1 break-all text-xs text-[var(--settings-text-secondary)]">{plugin.pluginId}</p>
              </div>
            </div>
            <div className="flex shrink-0 flex-wrap justify-end gap-1">
              <span className={settingsPill}>{t(plugin.source === 'official' ? 'settings.pluginOfficial' : 'settings.pluginCommunity')}</span>
              <span className={plugin.status === 'installed' ? settingsPill : settingsPillWarning}>
                {t(plugin.status === 'installed' ? 'settings.pluginInstalled' : 'settings.pluginMissing')}
              </span>
            </div>
          </div>
          <section className="flex flex-col gap-1 border-t border-border-subtle pt-sm">
            <h4 className="text-xs font-semibold uppercase tracking-wide text-[var(--settings-text-secondary)]">
              {t('settings.pluginPermissions')}
            </h4>
            {plugin.capabilities.map((capability) => (
              <div key={capability} className="flex flex-col gap-0.5 rounded-sm bg-raised p-sm">
                <span className="font-mono text-xs text-white">{capability}</span>
                <span className="text-xs text-[var(--settings-text-secondary)]">
                  {capability === 'library.discovery'
                    ? t('settings.pluginDiscoveryPermission')
                    : t('settings.pluginUnknownPermission')}
                </span>
              </div>
            ))}
          </section>
        </article>
      ))}
      <p className="text-xs text-[var(--settings-text-muted)]">{t('settings.pluginCommunityNote')}</p>
    </div>
  );
};
