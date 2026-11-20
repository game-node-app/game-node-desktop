import { IconAlertTriangle } from '@tabler/icons-react';
import type { Notice } from '../../api/types';
import { useI18n } from '../../i18n';
import { Banner } from '../../components/ui/Banner';

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

/** A sign-in problem, rendered with the shared Banner. */
export function NoticeBanner({ notice, className, onDismiss }: NoticeBannerProps) {
  const { t } = useI18n();

  if (!notice) {
    return null;
  }

  const msgid = NOTICE_MSGID_MAP[notice];
  const message = msgid ? t(msgid) : String(notice);

  return (
    <div data-testid="notice-banner" data-notice={notice} className={className}>
      <Banner
        tone="danger"
        icon={IconAlertTriangle}
        title={message}
        onDismiss={onDismiss}
        dismissLabel={onDismiss ? t('auth.dismissNotice') : undefined}
      />
    </div>
  );
}
