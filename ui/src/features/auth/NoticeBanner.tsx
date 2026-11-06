import { IconX } from '@tabler/icons-react';
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
      className={`flex items-center justify-between gap-sm rounded-md border border-danger bg-danger-subtle px-md py-sm text-sm leading-[1.4] text-text-default ${className ?? ''}`}
    >
      <span>{message}</span>
      {onDismiss && (
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss notice"
          className="cursor-pointer border-0 bg-transparent px-[6px] py-[2px] text-sm text-text-dimmed transition-colors hover:text-text-default"
        >
          <IconX size={14} stroke={2} aria-hidden="true" />
        </button>
      )}
    </div>
  );
}
