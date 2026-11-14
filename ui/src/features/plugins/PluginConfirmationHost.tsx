import React, { useCallback, useEffect, useState } from 'react';
import { AppModal } from '../../components/AppModal';
import {
  cleanConfirmation,
  commands,
  shortFingerprint,
  subscribeToPluginConfirmations,
  subscribeToPluginConfirmationsClosed,
  type PluginConfirmation,
} from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonPrimary, settingsButtonSecondary } from '../settings/settingsClasses';
import { PlainModalScrollArea } from '../settings/PlainModalScrollArea';

/** What is asked, in the user's language. Everything the plugin supplied is shown as text. */
export const PluginConfirmationDialog: React.FC<{
  confirmation: PluginConfirmation;
  waiting: number;
  onAnswer: (approved: boolean, remember: boolean) => void;
}> = ({ confirmation, waiting, onAnswer }) => {
  const { t } = useI18n();
  const [remember, setRemember] = useState(false);
  const launching = confirmation.operation === 'launch';
  const game = confirmation.gameName || t('plugins.confirm.unnamedGame');
  const fingerprint = shortFingerprint(confirmation.publisherFingerprint);

  // A new request starts with remembering off.
  useEffect(() => {
    setRemember(false);
  }, [confirmation.id]);

  return (
    <AppModal.Root
      opened
      onClose={() => onAnswer(false, false)}
      centered
      zIndex={1020}
      scrollAreaComponent={PlainModalScrollArea}
      withinPortal
    >
      <AppModal.Overlay backgroundOpacity={0.65} />
      <AppModal.Content
        classNames={{
          content:
            'w-[90%] max-w-[520px] flex-none rounded-md border border-border-subtle bg-secondary p-6 text-white shadow-alert',
        }}
      >
        <div
          role="alertdialog"
          aria-labelledby="plugin-confirm-title"
          aria-describedby="plugin-confirm-body"
          className="flex flex-col gap-md"
        >
          <h2 id="plugin-confirm-title" className="text-lg font-bold text-white">
            {launching ? t('plugins.confirm.launchTitle') : t('plugins.confirm.installTitle')}
          </h2>
          <p id="plugin-confirm-body" className="text-sm text-white">
            {launching
              ? t('plugins.confirm.launchBody', { plugin: confirmation.pluginId, game })
              : t('plugins.confirm.installBody', { plugin: confirmation.pluginId, game })}
          </p>
          <dl className="grid grid-cols-[auto_1fr] gap-x-md gap-y-1 text-xs text-[var(--settings-text-secondary)]">
            <dt>{t('plugins.confirm.pluginLabel')}</dt>
            <dd className="break-all text-white">{confirmation.pluginId}</dd>
            {fingerprint && (
              <>
                <dt>{t('plugins.confirm.publisherLabel')}</dt>
                <dd className="break-all text-white">{fingerprint}</dd>
              </>
            )}
            <dt>{t('plugins.confirm.gameLabel')}</dt>
            <dd className="break-words text-white">{game}</dd>
          </dl>
          <p className="text-xs text-[var(--settings-text-secondary)]">
            {launching ? t('plugins.confirm.launchNote') : t('plugins.confirm.installNote')}
          </p>
          <label className="flex items-start gap-compact text-sm text-white">
            <input
              type="checkbox"
              className="mt-[3px]"
              checked={remember}
              onChange={(event) => setRemember(event.target.checked)}
            />
            <span>
              {t('plugins.confirm.remember')}
              <span className="block text-xs text-[var(--settings-text-secondary)]">
                {t('plugins.confirm.rememberHint')}
              </span>
            </span>
          </label>
          {waiting > 0 && (
            <p role="status" className="text-xs text-[var(--settings-text-secondary)]">
              {t('plugins.confirm.moreWaiting', { count: String(waiting) })}
            </p>
          )}
          <div className="mt-compact flex justify-end gap-[10px]">
            <button
              // Cancel is the safe default: the modal puts the focus here when it opens.
              data-autofocus
              type="button"
              className={settingsButtonSecondary}
              onClick={() => onAnswer(false, false)}
            >
              {t('plugins.confirm.decline')}
            </button>
            <button
              type="button"
              className={settingsButtonPrimary}
              onClick={() => onAnswer(true, remember)}
            >
              {t('plugins.confirm.allow')}
            </button>
          </div>
        </div>
      </AppModal.Content>
    </AppModal.Root>
  );
};

/**
 * Asks the user about what plugins want to start. Requests queue; the first is shown, one at a
 * time, until it is answered or the host says it stopped waiting.
 */
export const PluginConfirmationHost: React.FC = () => {
  const [queue, setQueue] = useState<PluginConfirmation[]>([]);

  useEffect(() => {
    let active = true;
    const unlisten: Array<() => void> = [];
    const keep = (fn: () => void) => {
      if (active) unlisten.push(fn);
      else fn();
    };
    subscribeToPluginConfirmations((raw) => {
      const confirmation = cleanConfirmation(raw);
      if (!active || !confirmation) return;
      setQueue((current) =>
        current.some((item) => item.id === confirmation.id) ? current : [...current, confirmation],
      );
    })
      .then(keep)
      .catch(() => {
        // A plain browser without the event bridge has nothing to confirm.
      });
    subscribeToPluginConfirmationsClosed((id) => {
      if (active) setQueue((current) => current.filter((item) => item.id !== id));
    })
      .then(keep)
      .catch(() => {});
    return () => {
      active = false;
      unlisten.forEach((fn) => fn());
    };
  }, []);

  const answer = useCallback(
    (id: string, approved: boolean, remember: boolean) => {
      setQueue((current) => current.filter((item) => item.id !== id));
      // Remembering only goes with an approval.
      void commands.pluginConfirmationAnswer(id, approved, approved && remember);
    },
    [],
  );

  const current = queue[0];
  if (!current) return null;
  return (
    <PluginConfirmationDialog
      key={current.id}
      confirmation={current}
      waiting={queue.length - 1}
      onAnswer={(approved, remember) => answer(current.id, approved, remember)}
    />
  );
};
