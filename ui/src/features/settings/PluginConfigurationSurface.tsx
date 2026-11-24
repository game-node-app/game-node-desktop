import React, { useState } from 'react';
import { commands } from '../../api';
import type { PluginInfo } from '../../api';
import type { ConfigurationChoice } from '../../api/pluginSettings';
import { useI18n } from '../../i18n';
import { PluginDeclarativeSettings } from './PluginDeclarativeSettings';
import { settingsRowDescription } from './settingsClasses';

type RequestState = 'idle' | 'opening' | 'focusing' | 'open' | 'focused' | 'unavailable' | 'denied' | 'timedOut' | 'failed';

export const PluginConfigurationSurface: React.FC<{
  plugin: PluginInfo;
  choice?: ConfigurationChoice;
  loading?: boolean;
}> = ({ plugin, choice, loading = false }) => {
  const { t } = useI18n();
  const [requestState, setRequestState] = useState<RequestState>('idle');
  const [windowOpen, setWindowOpen] = useState(false);
  const [requesting, setRequesting] = useState(false);
  const enabled = plugin.status === 'installed' && plugin.session === 'active';

  const configure = async () => {
    if (!enabled || requesting) return;
    const focus = windowOpen;
    setRequesting(true);
    setRequestState(focus ? 'focusing' : 'opening');
    try {
      const result = await commands.pluginConfigurationOpen(plugin.providerId, focus);
      setRequestState(result.status);
      const accepted = result.status === 'open' || result.status === 'focused';
      setWindowOpen(accepted);
    } catch {
      setRequestState('unavailable');
      setWindowOpen(false);
    } finally {
      setRequesting(false);
    }
  };

  if (!enabled) return <p className={settingsRowDescription} role="status">{t('plugins.configure.disabled')}</p>;
  if (loading) return <p className={settingsRowDescription} role="status">{t('plugins.configure.loading')}</p>;
  if (!choice) return <p className={settingsRowDescription} role="status">{t('plugins.configure.unavailable')}</p>;

  if (choice === 'external') return <div className="mt-compact grid gap-1">
    <button type="button" className="w-fit rounded-sm border border-brand bg-brand px-sm py-1 text-sm text-white hover:brightness-110" disabled={requesting} onClick={() => void configure()}>
      {requesting ? t(requestState === 'focusing' ? 'plugins.configure.focusing' : 'plugins.configure.opening') : t('plugins.configure.action')}
    </button>
    {requestState !== 'idle' && <p className={settingsRowDescription} role="status">{t(`plugins.configure.status.${requestState}`)}</p>}
  </div>;
  if (choice === 'form') return <PluginDeclarativeSettings providerId={plugin.providerId} />;
  if (choice === 'denied') return <p className={settingsRowDescription} role="status">{t('plugins.settings.denied')}</p>;
  return <p className={settingsRowDescription} role="status">{t('plugins.settings.absent')}</p>;
};
