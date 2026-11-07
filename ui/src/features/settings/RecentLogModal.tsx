import React, { useEffect, useState } from 'react';
import { FloatingModal } from '../../components/FloatingModal';
import { commands } from '../../api';
import { copyText } from '../../diagnostics/crashCopy';
import { useI18n } from '../../i18n';
import { settingsButtonPrimary, settingsButtonSecondary } from './settingsClasses';

export interface RecentLogModalProps {
  onClose: () => void;
}

/** The tail of the desktop application log, with a one-click copy for bug reports. */
export const RecentLogModal: React.FC<RecentLogModalProps> = ({ onClose }) => {
  const { t } = useI18n();
  const [text, setText] = useState<string>('');
  const [status, setStatus] = useState<'loading' | 'ready' | 'empty' | 'failed'>('loading');
  const [copyStatus, setCopyStatus] = useState<'idle' | 'copied' | 'failed'>('idle');

  useEffect(() => {
    let cancelled = false;
    commands
      .getRecentLogs()
      .then((value) => {
        if (cancelled) return;
        setText(value);
        setStatus(value.trim() ? 'ready' : 'empty');
      })
      .catch(() => {
        if (!cancelled) setStatus('failed');
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleCopy = async () => {
    const ok = await copyText(text);
    setCopyStatus(ok ? 'copied' : 'failed');
  };

  return (
    <FloatingModal.Root
      opened
      onClose={onClose}
      geometryKey="recent-log"
      defaultSize={{ width: 840, height: 620 }}
      minSize={{ width: 520, height: 380 }}
      zIndex={1010}
      closeOnClickOutside
    >
      <FloatingModal.Overlay backgroundOpacity={0.65} />
      <FloatingModal.Content
        aria-label={t('Recent log') || 'Recent log'}
        className="flex flex-col bg-secondary text-white"
      >
        <FloatingModal.Header>
          <FloatingModal.Title>{t('Recent log') || 'Recent log'}</FloatingModal.Title>
          <FloatingModal.CloseButton data-autofocus onClick={onClose} />
        </FloatingModal.Header>

        <FloatingModal.Body className="flex min-h-0 flex-col gap-sm px-control-inline py-lg">
          <span className="text-xs text-[var(--settings-text-secondary)]">
            {t('Paste this log when reporting a problem. Cookies, tokens and passwords are never written to it.') ||
              'Paste this log when reporting a problem. Cookies, tokens and passwords are never written to it.'}
          </span>
          <textarea
            readOnly
            aria-label={t('Recent log') || 'Recent log'}
            value={
              status === 'loading'
                ? t('Loading…') || 'Loading…'
                : status === 'failed'
                  ? t('The log could not be read.') || 'The log could not be read.'
                  : status === 'empty'
                    ? t('The log is empty.') || 'The log is empty.'
                    : text
            }
            className="min-h-[280px] flex-1 resize-none rounded-sm border border-border-subtle bg-[var(--settings-bg-base)] p-compact font-mono text-xs leading-normal text-[var(--settings-text-secondary)] focus-visible:ring-2 focus-visible:ring-brand focus-visible:outline-none"
            onFocus={(event) => event.currentTarget.select()}
          />
          <span role="status" aria-live="polite" className="text-xs text-[var(--settings-text-muted)]">
            {copyStatus === 'copied'
              ? t('Copied to the clipboard.') || 'Copied to the clipboard.'
              : copyStatus === 'failed'
                ? t('Could not copy to the clipboard.') || 'Could not copy to the clipboard.'
                : ''}
          </span>
        </FloatingModal.Body>

        <FloatingModal.Footer className="bg-[var(--settings-bg-base)]">
          <button
            type="button"
            className={settingsButtonSecondary}
            onClick={handleCopy}
            disabled={status !== 'ready'}
          >
            {t('Copy log') || 'Copy log'}
          </button>
          <button type="button" className={settingsButtonPrimary} onClick={onClose}>
            {t('Close') || 'Close'}
          </button>
        </FloatingModal.Footer>

        <FloatingModal.ResizeHandle />
      </FloatingModal.Content>
    </FloatingModal.Root>
  );
};
