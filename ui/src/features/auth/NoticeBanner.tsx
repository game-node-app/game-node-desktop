import type { Notice } from '../../api/types';
import { useI18n } from '../../i18n';

export const NOTICE_MSGID_MAP: Record<Notice, string> = {
  browserLaunchFailed: 'Could not open your browser. Please try again.',
  signInUnavailable: 'Could not start sign-in. Please try again.',
  providerRejected: 'Sign-in was rejected. Please try again.',
  attemptExpired: 'Sign-in expired. Please try again.',
  signInCancelled: 'Sign-in cancelled.',
  signInRejected: 'The session was rejected. Please sign in again.',
  connectionProblem: 'Could not connect. Your stored session is preserved; retry when online.',
  sessionRejected: 'Your stored session expired. Please sign in again.',
  credentialUnreadable: 'Could not read your session from credential storage.',
  persistFailed: 'Could not save your session. Sign-in will not survive restart.',
  signOutNotConfirmed: 'Signed out locally. Server sign-out could not be confirmed.',
  credentialDeleteFailed: 'Could not remove the saved credential. Check credential storage before restarting.',
};

export interface NoticeBannerProps {
  notice?: Notice | null;
  className?: string;
  onDismiss?: () => void;
}

export function NoticeBanner({ notice, className, onDismiss }: NoticeBannerProps) {
  const { t } = useI18n();

  if (!notice) {
    return null;
  }

  const msgid = NOTICE_MSGID_MAP[notice];
  const message = msgid ? t(msgid) : String(notice);

  return (
    <div
      role="alert"
      data-testid="notice-banner"
      data-notice={notice}
      className={className}
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        gap: '12px',
        padding: '12px 16px',
        backgroundColor: 'var(--color-danger-subtle, rgba(229, 72, 77, 0.12))',
        border: '1px solid var(--color-danger, #E5484D)',
        borderRadius: 'var(--radius-md, 8px)',
        color: 'var(--color-text-default, #D9D9D9)',
        fontSize: 'var(--font-size-sm, 14px)',
        lineHeight: 1.4,
      }}
    >
      <span>{message}</span>
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss notice"
          style={{
            background: 'none',
            border: 'none',
            color: 'var(--color-text-dimmed, #808080)',
            cursor: 'pointer',
            padding: '2px 6px',
            fontSize: '14px',
          }}
        >
          ✕
        </button>
      )}
    </div>
  );
}
