import React, { useEffect, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { AppModal } from '../../components/AppModal';
import {
  cleanRootConfirmation,
  cleanRootSelection,
  commands,
  isTauriRuntime,
  shortFingerprint,
  subscribeToRootConfirmations,
  subscribeToRootPromptsClosed,
  subscribeToRootSelections,
  type RootConfirmationPrompt,
  type RootSelectionPrompt,
} from '../../api';
import { useI18n } from '../../i18n';
import { settingsButtonPrimary, settingsButtonSecondary } from '../settings/settingsClasses';

function operationLabel(operation: string, t: (key: string) => string): string {
  return t(`plugins.folder.operation.${operation}`);
}

export const PluginFolderApprovalHost: React.FC = () => {
  const { t } = useI18n();
  const [selections, setSelections] = useState<RootSelectionPrompt[]>([]);
  const [confirmations, setConfirmations] = useState<RootConfirmationPrompt[]>([]);
  const [selectedPath, setSelectedPath] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const selection = selections[0];
  const confirmation = confirmations[0];
  const fingerprint = shortFingerprint(selection?.publisherFingerprint ?? confirmation?.publisherFingerprint ?? '');

  useEffect(() => {
    let unlistenSelections: (() => void) | undefined;
    let unlistenConfirmations: (() => void) | undefined;
    let unlistenClosed: (() => void) | undefined;
    let cancelled = false;
    void Promise.all([
      subscribeToRootSelections((raw) => {
        const prompt = cleanRootSelection(raw);
        if (prompt) setSelections((current) => current.some((item) => item.id === prompt.id) ? current : [...current, prompt]);
      }).then((dispose) => {
        if (cancelled) dispose(); else unlistenSelections = dispose;
      }),
      subscribeToRootConfirmations((raw) => {
        const prompt = cleanRootConfirmation(raw);
        if (prompt) setConfirmations((current) => current.some((item) => item.id === prompt.id) ? current : [...current, prompt]);
      }).then((dispose) => {
        if (cancelled) dispose(); else unlistenConfirmations = dispose;
      }),
      subscribeToRootPromptsClosed((id) => {
        setSelections((current) => current.filter((item) => item.id !== id));
        setConfirmations((current) => current.filter((item) => item.id !== id));
      }).then((dispose) => {
        if (cancelled) dispose(); else unlistenClosed = dispose;
      }),
    ]);
    return () => {
      cancelled = true;
      unlistenSelections?.();
      unlistenConfirmations?.();
      unlistenClosed?.();
    };
  }, []);

  useEffect(() => {
    setSelectedPath(selection?.preselected ?? '');
    setError(false);
  }, [selection?.id, selection?.preselected]);

  const closeSelection = (id: string) => setSelections((current) => current.filter((item) => item.id !== id));
  const closeConfirmation = (id: string) => setConfirmations((current) => current.filter((item) => item.id !== id));

  const answerSelection = async (path: string | null) => {
    if (!selection || busy) return;
    setBusy(true);
    setError(false);
    try {
      await commands.pluginFolderSelectionAnswer(selection.id, path);
      closeSelection(selection.id);
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async () => {
    if (!isTauriRuntime()) {
      setError(true);
      return;
    }
    try {
      const result = await open({ directory: true, multiple: false, title: t('plugins.folder.chooseTitle') });
      if (typeof result === 'string') setSelectedPath(result);
    } catch {
      setError(true);
    }
  };

  const answerConfirmation = async (approved: boolean) => {
    if (!confirmation || busy) return;
    setBusy(true);
    setError(false);
    try {
      await commands.pluginFolderApprovalAnswer(confirmation.id, approved);
      closeConfirmation(confirmation.id);
    } catch {
      setError(true);
    } finally {
      setBusy(false);
    }
  };

  if (selection) {
    const logicalRoot = selection.kind === 'library';
    return (
      <AppModal.Root opened onClose={() => void answerSelection(null)} centered zIndex={1020}>
        <AppModal.Overlay />
        <AppModal.Content className="w-[94%] max-w-[760px] bg-secondary p-6 text-text-default shadow-alert">
          <AppModal.Header className="border-0 pb-4">
            <AppModal.Title>{t('plugins.folder.selectTitle')}</AppModal.Title>
            <AppModal.CloseButton aria-label={t('plugins.folder.cancel')} onClick={() => void answerSelection(null)} />
          </AppModal.Header>
          <AppModal.Body className="gap-4">
            <p className="text-sm leading-relaxed">{t('plugins.folder.requestedBy', { plugin: selection.pluginId, target: selection.installationName })}</p>
            <dl className="grid grid-cols-[auto_1fr] gap-x-5 gap-y-2 text-sm text-text-dimmed">
              <dt>{t('plugins.folder.publisher')}</dt><dd className="break-all text-text-default">{fingerprint || t('plugins.folder.unknownPublisher')}</dd>
              <dt>{t('plugins.folder.kind')}</dt><dd className="text-text-default">{t(`plugins.folder.kind.${selection.kind}`)}</dd>
              <dt>{t('plugins.folder.requested')}</dt><dd className="flex flex-wrap gap-1">{selection.operations.map((op) => <span key={op} className="rounded bg-raised px-1.5 py-0.5 text-text-default">{operationLabel(op, t)}</span>)}</dd>
            </dl>
            {selection.previouslyApproved && (
              <div className="rounded border border-border-subtle bg-primary/40 p-4 text-sm">
                <p className="font-semibold">{t('plugins.folder.wideningTitle')}</p>
                <p className="mt-1 break-all">{t('plugins.folder.alreadyApproved', { root: selection.previouslyApproved.root, operations: selection.previouslyApproved.operations.map((op) => operationLabel(op, t)).join(', ') })}</p>
                <p className="mt-1">{t('plugins.folder.addedOperations', { operations: selection.operations.filter((op) => !selection.previouslyApproved?.operations.includes(op)).map((op) => operationLabel(op, t)).join(', ') })}</p>
              </div>
            )}
            {logicalRoot && <p className="rounded border border-border-subtle bg-primary/40 p-4 text-sm">{t('plugins.folder.logicalRootDisclosure')}</p>}
            {selection.declared.length > 0 && (
              <div>
                <p className="text-sm font-semibold">{t('plugins.folder.suggestions')}</p>
                <ul className="mt-2 flex max-h-48 flex-col gap-2 overflow-y-auto">
                  {selection.declared.map((folder, index) => (
                    <li key={`${folder.path}-${index}`}>
                      <button type="button" className="w-full rounded border border-border-subtle p-3 text-left text-sm hover:bg-raised" onClick={() => setSelectedPath(folder.path)}>
                        <span className="block break-all text-text-default">{folder.path}</span>
                        <span className="mt-1 block text-text-dimmed">{folder.description} · {t('plugins.folder.declaredUnverified')}{folder.exists ? '' : ` · ${t('plugins.folder.notFound')}`}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {selection.suggested && <p className="text-sm text-text-dimmed">{t('plugins.folder.hostSuggestion', { path: selection.suggested })}</p>}
            <label className="flex flex-col gap-1.5 text-sm font-semibold">
              {t('plugins.folder.selectedPath')}
              <input className="rounded border border-border-subtle bg-primary px-3 py-2 font-normal text-text-default" value={selectedPath} onChange={(event) => setSelectedPath(event.target.value)} spellCheck={false} autoComplete="off" />
            </label>
            <div className="flex flex-wrap gap-2">
              <button type="button" className={settingsButtonSecondary} onClick={() => void pickFolder()}>{t('plugins.folder.chooseAnother')}</button>
              {selection.preselected && <button type="button" className={settingsButtonSecondary} onClick={() => setSelectedPath(selection.preselected ?? '')}>{t('plugins.folder.useSuggested')}</button>}
            </div>
            {error && <p role="alert" className="text-sm text-danger">{t('plugins.folder.error')}</p>}
            <div className="mt-3 flex justify-end gap-2">
              <button autoFocus data-autofocus type="button" className={settingsButtonSecondary} disabled={busy} onClick={() => void answerSelection(null)}>{t('plugins.folder.cancel')}</button>
              <button type="button" className={settingsButtonPrimary} disabled={busy || !selectedPath.trim()} onClick={() => void answerSelection(selectedPath.trim())}>{t('plugins.folder.review')}</button>
            </div>
          </AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>
    );
  }

  if (confirmation) {
    const logicalRoot = confirmation.kind === 'library';
    return (
      <AppModal.Root opened onClose={() => void answerConfirmation(false)} centered zIndex={1020}>
        <AppModal.Overlay />
        <AppModal.Content className="w-[94%] max-w-[760px] bg-secondary p-6 text-text-default shadow-alert">
          <AppModal.Header className="border-0 pb-4">
            <AppModal.Title>{t('plugins.folder.confirmTitle')}</AppModal.Title>
            <AppModal.CloseButton aria-label={t('plugins.folder.cancel')} onClick={() => void answerConfirmation(false)} />
          </AppModal.Header>
          <AppModal.Body className="gap-3">
            <p className="text-sm">{t('plugins.folder.requestedBy', { plugin: confirmation.pluginId, target: confirmation.installationName })}</p>
            <dl className="grid grid-cols-[auto_1fr] gap-x-5 gap-y-2 text-sm text-text-dimmed">
              <dt>{t('plugins.folder.publisher')}</dt><dd className="break-all text-text-default">{fingerprint || t('plugins.folder.unknownPublisher')}</dd>
              <dt>{t('plugins.folder.kind')}</dt><dd className="text-text-default">{t(`plugins.folder.kind.${confirmation.kind}`)}</dd>
              <dt>{t('plugins.folder.folder')}</dt><dd className="break-all text-text-default">{confirmation.root}</dd>
              <dt>{t('plugins.folder.operationsLabel')}</dt><dd className="flex flex-wrap gap-1">{confirmation.operations.map((op) => <span key={op} className="rounded bg-raised px-1.5 py-0.5 text-text-default">{operationLabel(op, t)}</span>)}</dd>
              <dt>{t('plugins.folder.origin')}</dt><dd className="text-text-default">{confirmation.declaredByPlugin ? t('plugins.folder.declaredUnverified') : t('plugins.folder.userSelected')}</dd>
            </dl>
            {logicalRoot ? (
              <p className="rounded border border-border-subtle bg-primary/40 p-4 text-sm">{t('plugins.folder.logicalRootDisclosure')}</p>
            ) : (
              <div>
                <p className="text-sm font-semibold">{t('plugins.folder.affectedGames')}</p>
                {confirmation.affected.length > 0 ? <ul className="mt-1 max-h-32 list-disc overflow-y-auto pl-5 text-xs">{confirmation.affected.map((game, index) => <li key={`${game.gameId ?? 'none'}-${index}`}>{game.name}</li>)}</ul> : <p className="mt-1 text-xs text-text-dimmed">{t('plugins.folder.noAffectedGames')}</p>}
              </div>
            )}
            <p className="text-sm text-text-dimmed">{t('plugins.folder.confirmDisclosure')}</p>
            {error && <p role="alert" className="text-sm text-danger">{t('plugins.folder.error')}</p>}
            <div className="mt-3 flex justify-end gap-2">
              <button autoFocus data-autofocus type="button" className={settingsButtonSecondary} disabled={busy} onClick={() => void answerConfirmation(false)}>{t('plugins.folder.cancel')}</button>
              <button type="button" className={settingsButtonPrimary} disabled={busy} onClick={() => void answerConfirmation(true)}>{t('plugins.folder.approve')}</button>
            </div>
          </AppModal.Body>
        </AppModal.Content>
      </AppModal.Root>
    );
  }
  return null;
};
