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

const launchDifferenceKeys: Record<string, string> = {
  'recipe-selected': 'plugins.confirm.difference.recipe',
  'custom-target': 'plugins.confirm.difference.target',
  'known-components': 'plugins.confirm.difference.components',
  'custom-arguments': 'plugins.confirm.difference.arguments',
  'custom-working-directory': 'plugins.confirm.difference.workingDirectory',
  'environment-changes': 'plugins.confirm.difference.environment',
};

/** What is asked, in the user's language. Everything the plugin supplied is shown as text. */
export const PluginConfirmationDialog: React.FC<{
  confirmation: PluginConfirmation;
  waiting: number;
  onAnswer: (approved: boolean, remember: boolean) => void;
}> = ({ confirmation, waiting, onAnswer }) => {
  const { t } = useI18n();
  const [remember, setRemember] = useState(false);
  const [revealEnvironment, setRevealEnvironment] = useState(false);
  const launching = confirmation.operation === 'launch';
  const customLaunching = confirmation.operation === 'custom_launch';
  const launchPlan = confirmation.launchPlan;
  const restoring = confirmation.operation === 'backup_restore';
  const game = confirmation.gameName || t('plugins.confirm.unnamedGame');
  const fingerprint = shortFingerprint(confirmation.publisherFingerprint);

  // A new request starts with remembering off.
  useEffect(() => {
    setRemember(false);
    setRevealEnvironment(false);
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
            {launchPlan
              ? t('plugins.confirm.planTitle')
              : restoring
                ? t('plugins.confirm.restoreTitle')
                : launching || customLaunching
                  ? t('plugins.confirm.launchTitle')
                  : t('plugins.confirm.installTitle')}
          </h2>
          <p id="plugin-confirm-body" className="text-sm text-white">
            {launchPlan
              ? t('plugins.confirm.planBody', { plugin: confirmation.pluginId, game })
              : restoring
              ? t('plugins.confirm.restoreBody', { plugin: confirmation.pluginId, game })
              : launching || customLaunching
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
            {restoring ? t('plugins.confirm.restoreNote') : launching || customLaunching ? t('plugins.confirm.launchNote') : t('plugins.confirm.installNote')}
          </p>
          {launchPlan && (
            <section
              aria-label={t('plugins.confirm.planTitle')}
              className="flex max-h-[45vh] flex-col gap-compact overflow-auto rounded-sm border border-border-subtle bg-raised p-sm text-xs text-white"
              data-testid="launch-plan-review"
            >
              <dl className="grid grid-cols-[auto_1fr] gap-x-md gap-y-1">
                <dt>{t('plugins.confirm.source')}</dt>
                <dd className="break-all">{launchPlan.sourceId}</dd>
                <dt>{t('plugins.confirm.target')}</dt>
                <dd className="break-all font-mono">{launchPlan.target}</dd>
                <dt>{t('plugins.confirm.packageVersion')}</dt>
                <dd className="break-all">{launchPlan.requesterPackageVersion}</dd>
                <dt>{t('plugins.confirm.approvalFingerprint')}</dt>
                <dd className="break-all font-mono">{launchPlan.fingerprint}</dd>
              </dl>
              {launchPlan.components.length > 0 && (
                <div>
                  <h3 className="font-semibold">{t('plugins.confirm.components')}</h3>
                  <ol className="list-decimal pl-5">
                    {launchPlan.components.map((component, index) => (
                      <li key={`${component.kind}:${component.id}:${index}`} className="break-all">
                        {component.kind}: {component.id} · {component.version} · r{component.revision} · {component.ownerPluginId}
                      </li>
                    ))}
                  </ol>
                </div>
              )}
              {launchPlan.arguments.length > 0 && (
                <div>
                  <h3 className="font-semibold">{t('plugins.confirm.arguments')}</h3>
                  <ol className="list-decimal pl-5">
                    {launchPlan.arguments.map((argument, index) => (
                      <li key={index} className="break-all font-mono">{argument}</li>
                    ))}
                  </ol>
                </div>
              )}
              {launchPlan.workingDirectory && (
                <div>
                  <h3 className="font-semibold">{t('plugins.confirm.workingDirectory')}</h3>
                  <code className="break-all">{launchPlan.workingDirectory}</code>
                </div>
              )}
              {(Object.keys(launchPlan.environmentSet).length > 0 || launchPlan.environmentUnset.length > 0) && (
                <div>
                  <h3 className="font-semibold">{t('plugins.confirm.environmentChanges')}</h3>
                  <button
                    type="button"
                    aria-expanded={revealEnvironment}
                    aria-controls="launch-plan-environment-values"
                    onClick={() => setRevealEnvironment((shown) => !shown)}
                    className={settingsButtonSecondary}
                  >
                    {revealEnvironment
                      ? t('plugins.confirm.hideEnvironmentValues')
                      : t('plugins.confirm.revealEnvironmentValues')}
                  </button>
                  <ul id="launch-plan-environment-values" className="mt-compact">
                    {Object.entries(launchPlan.environmentSet).map(([key, value]) => (
                      <li key={key} className="break-all font-mono">
                        {key}={revealEnvironment ? value : '••••••'}
                      </li>
                    ))}
                    {launchPlan.environmentUnset.map((key) => (
                      <li key={key} className="break-all font-mono">{key} ({t('plugins.confirm.unset')})</li>
                    ))}
                  </ul>
                </div>
              )}
              {launchPlan.differences.length > 0 && (
                <div>
                  <h3 className="font-semibold">{t('plugins.confirm.differences')}</h3>
                  <ul className="list-disc pl-5">
                    {launchPlan.differences.map((difference, index) => (
                      <li key={index}>{t(launchDifferenceKeys[difference] ?? 'plugins.confirm.difference.other')}</li>
                    ))}
                  </ul>
                </div>
              )}
            </section>
          )}
          {restoring && (
            <ul aria-label={t('plugins.confirm.restoreTargets')} className="max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs text-white">
              {(confirmation.details ?? []).map((detail, index) => (
                <li key={index}>{detail.replace(/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/g,
                  (char) => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`)}</li>
              ))}
            </ul>
          )}
          {!restoring && <label className="flex items-start gap-compact text-sm text-white">
            <input
              type="checkbox"
              className="mt-[3px]"
              checked={remember}
              onChange={(event) => setRemember(event.target.checked)}
            />
            <span>
              {t(launchPlan ? 'plugins.confirm.rememberExactPlan' : 'plugins.confirm.remember')}
              <span className="block text-xs text-[var(--settings-text-secondary)]">
                {t('plugins.confirm.rememberHint')}
              </span>
            </span>
          </label>}
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
              onClick={() => onAnswer(true, !restoring && remember)}
            >
              {launchPlan ? t('plugins.confirm.runOnce') : t('plugins.confirm.allow')}
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
