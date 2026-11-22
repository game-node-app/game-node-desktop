import React, { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { AppModal } from '../../components/AppModal';
import {
  cleanRuntimeSelection,
  commands,
  isTauriRuntime,
  shortFingerprint,
  subscribeToRuntimePromptsClosed,
  subscribeToRuntimeSelections,
  type RuntimeSelectionPrompt,
} from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonPrimary, settingsButtonSecondary } from '../settings/settingsClasses';

function candidatePackage(candidate: RuntimeSelectionPrompt['candidates'][number]): string {
  return [candidate.packageManager, candidate.packageId, candidate.version]
    .filter((value) => value.length > 0)
    .join(' · ');
}

export const PluginRuntimeApprovalHost: React.FC = () => {
  const { t } = useI18n();
  const [selections, setSelections] = useState<RuntimeSelectionPrompt[]>([]);
  const [selectedPath, setSelectedPath] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const selection = selections[0];
  const fingerprint = shortFingerprint(selection?.publisherFingerprint ?? '');

  useEffect(() => {
    let unlistenSelections: (() => void) | undefined;
    let unlistenClosed: (() => void) | undefined;
    let cancelled = false;
    void Promise.all([
      subscribeToRuntimeSelections((raw) => {
        const prompt = cleanRuntimeSelection(raw);
        if (prompt) setSelections((current) => current.some((item) => item.id === prompt.id) ? current : [...current, prompt]);
      }).then((dispose) => {
        if (cancelled) dispose(); else unlistenSelections = dispose;
      }),
      subscribeToRuntimePromptsClosed((id) => {
        setSelections((current) => current.filter((item) => item.id !== id));
      }).then((dispose) => {
        if (cancelled) dispose(); else unlistenClosed = dispose;
      }),
    ]);
    return () => {
      cancelled = true;
      unlistenSelections?.();
      unlistenClosed?.();
    };
  }, []);

  useEffect(() => {
    setSelectedPath(selection?.candidates[0]?.path ?? '');
    setError(false);
  }, [selection?.id]);

  const close = (id: string) => setSelections((current) => current.filter((item) => item.id !== id));

  const answer = async (path: string | null) => {
    if (!selection || busy) return;
    setBusy(true);
    setError(false);
    try {
      await commands.pluginRuntimeSelectionAnswer(selection.id, path);
      close(selection.id);
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    if (!isTauriRuntime()) {
      setError(true);
      return;
    }
    try {
      const result = await open({ directory: false, multiple: false, title: t('plugins.runtime.chooseTitle') });
      if (typeof result === 'string') setSelectedPath(result);
    } catch {
      setError(true);
    }
  };

  if (!selection) return null;

  return (
    <AppModal.Root opened onClose={() => void answer(null)} centered zIndex={1020}>
      <AppModal.Overlay />
      <AppModal.Content className="w-[92%] max-w-[680px] bg-secondary p-5 text-text-default shadow-alert">
        <AppModal.Header className="border-0 pb-3">
          <AppModal.Title>{t('plugins.runtime.title')}</AppModal.Title>
          <AppModal.CloseButton aria-label={t('plugins.runtime.cancel')} onClick={() => void answer(null)} />
        </AppModal.Header>
        <AppModal.Body className="gap-3">
          <p className="text-sm">
            {t('plugins.runtime.requestedBy', {
              plugin: selection.pluginId,
              runtime: selection.runtimeName || selection.pluginId,
            })}
          </p>
          <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-xs text-text-dimmed">
            <dt>{t('plugins.runtime.publisher')}</dt>
            <dd className="break-all text-text-default">{fingerprint || t('plugins.runtime.unknownPublisher')}</dd>
            <dt>{t('plugins.runtime.runtime')}</dt>
            <dd className="text-text-default">
              {selection.runtimeName}
              {selection.runtimeVersion ? ` · ${selection.runtimeVersion}` : ''}
            </dd>
            <dt>{t('plugins.runtime.platform')}</dt>
            <dd className="text-text-default">{selection.platform || '—'}</dd>
            <dt>{t('plugins.runtime.formats')}</dt>
            <dd className="text-text-default">{selection.supportedFormats.join(', ') || '—'}</dd>
          </dl>
          <p className="rounded border border-border-subtle bg-primary/40 p-3 text-xs">
            {t('plugins.runtime.disclosure')}
          </p>
          <div>
            <p className="text-xs font-semibold">{t('plugins.runtime.suggestions')}</p>
            {selection.candidates.length > 0 ? (
              <ul className="mt-1 flex max-h-52 flex-col gap-1 overflow-y-auto">
                {selection.candidates.map((candidate, index) => {
                  const selected = selectedPath === candidate.path;
                  return (
                    <li key={`${candidate.path}-${index}`}>
                      <button
                        type="button"
                        aria-pressed={selected}
                        className={`w-full rounded border p-2 text-left text-xs ${selected ? 'border-brand bg-raised' : 'border-border-subtle hover:bg-raised'}`}
                        onClick={() => setSelectedPath(candidate.path)}
                      >
                        <span className="block text-text-default">{candidate.displayName || candidate.path}</span>
                        {candidatePackage(candidate) && <span className="mt-1 block text-text-dimmed">{candidatePackage(candidate)}</span>}
                        <span className="mt-1 block break-all text-text-dimmed">{candidate.path}</span>
                      </button>
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="mt-1 text-xs text-text-dimmed">{t('plugins.runtime.noSuggestions')}</p>
            )}
          </div>
          <label className="flex flex-col gap-1 text-xs font-semibold">
            {t('plugins.runtime.selectedPath')}
            <input
              className="rounded border border-border-subtle bg-primary px-2 py-1.5 font-normal text-text-default"
              value={selectedPath}
              onChange={(event) => setSelectedPath(event.target.value)}
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <div className="flex flex-wrap gap-2">
            <button type="button" className={settingsButtonSecondary} onClick={() => void browse()}>
              {t('plugins.runtime.browse')}
            </button>
            {selection.hints.length > 0 && (
              <p className="self-center text-xs text-text-dimmed">
                {t('plugins.runtime.hints', { hints: selection.hints.join(', ') })}
              </p>
            )}
          </div>
          {error && <p role="alert" className="text-sm text-danger">{t('plugins.runtime.error')}</p>}
          <div className="mt-2 flex justify-end gap-2">
            <button autoFocus data-autofocus type="button" className={settingsButtonSecondary} disabled={busy} onClick={() => void answer(null)}>
              {t('plugins.runtime.cancel')}
            </button>
            <button type="button" className={settingsButtonPrimary} disabled={busy || !selectedPath.trim()} onClick={() => void answer(selectedPath.trim())}>
              {t('plugins.runtime.approve')}
            </button>
          </div>
        </AppModal.Body>
      </AppModal.Content>
    </AppModal.Root>
  );
};
