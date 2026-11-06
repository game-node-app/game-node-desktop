import { commands } from '../../api/commands';
import type { AuthView, Notice } from '../../api/types';
import { useAuth } from '../../api/useSnapshot';
import { useI18n } from '../../i18n';
import { NoticeBanner } from './NoticeBanner';

export interface SignedOutViewProps {
  auth?: AuthView;
  notice?: Notice | null;
  className?: string;
}

export function SignedOutView({
  auth: propAuth,
  notice: propNotice,
  className,
}: SignedOutViewProps = {}) {
  const authState = useAuth();
  const { t } = useI18n();

  const auth = propAuth !== undefined ? propAuth : authState.auth;
  const notice = propNotice !== undefined ? propNotice : authState.notice;

  const isPending =
    typeof auth === 'object' && auth !== null && 'pending' in auth;
  const isBusy =
    auth === 'starting' ||
    auth === 'verifying' ||
    auth === 'signingOut' ||
    isPending;
  const isRestoreFailed = auth === 'restoreFailed';

  const handleStartSignIn = (provider: 'google' | 'discord' | 'twitter') => {
    commands.dispatchAuth({ startSignIn: provider });
  };

  const handleCancelSignIn = () => {
    commands.dispatchAuth('cancelSignIn');
  };

  const handleRetryRestore = () => {
    commands.dispatchAuth('retryRestore');
  };

  const handleSignOut = () => {
    commands.dispatchAuth('signOut');
  };

  return (
    <div
      data-testid="signed-out-view"
      className={`flex min-h-full w-full flex-col items-center justify-center bg-body p-xl text-text-default ${className ?? ''}`}
    >
      <div className="flex w-full max-w-[440px] flex-col gap-[24px]">
        <NoticeBanner notice={notice} />

        <div className="text-center">
          <h1 className="m-0 mb-compact text-[24px] font-semibold text-text-default">
            {t('Welcome to GameNode')}
          </h1>
          <p className="m-0 text-sm text-text-dimmed">
            {t('Your next session starts here.')}
          </p>
        </div>

        <div className="flex flex-col gap-lg rounded-lg border border-border-subtle bg-surface px-control-inline py-xl shadow-modal">
          <div className="text-center">
            <h2 className="m-0 mb-[6px] text-lg font-medium text-text-default">
              {t('Make yourself at home.')}
            </h2>
            <p className="m-0 text-[13px] leading-[1.4] text-text-dimmed">
              {t('Sign in with your GameNode account in your browser.')}
            </p>
          </div>

          {!isBusy && !isRestoreFailed && (
            <div className="flex flex-col gap-sm">
              <button
                type="button"
                onClick={() => handleStartSignIn('google')}
                className="flex h-[40px] cursor-pointer items-center justify-center gap-xs rounded-md border-0 bg-brand text-sm font-medium text-on-primary transition-opacity hover:opacity-90"
              >
                <span>{t('Continue with Google')}</span>
              </button>

              <button
                type="button"
                onClick={() => handleStartSignIn('discord')}
                className="flex h-[40px] cursor-pointer items-center justify-center gap-xs rounded-md border border-border-subtle bg-transparent text-sm font-medium text-text-default transition-colors hover:bg-raised"
              >
                <span>{t('Continue with Discord')}</span>
              </button>

              <button
                type="button"
                onClick={() => handleStartSignIn('twitter')}
                className="flex h-[40px] cursor-pointer items-center justify-center gap-xs rounded-md border border-border-subtle bg-transparent text-sm font-medium text-text-default transition-colors hover:bg-raised"
              >
                <span>{t('Continue with Twitter')}</span>
              </button>
            </div>
          )}

          {isBusy && (
            <div className="flex flex-col items-center gap-md text-center">
              {isPending && (
                <p className="m-0 text-sm text-text-default">
                  {t('Complete sign-in in your browser.')}
                </p>
              )}
              {auth === 'verifying' && (
                <p className="m-0 text-sm text-text-dimmed">
                  {t('Verifying sign-in…')}
                </p>
              )}
              {auth === 'starting' && (
                <p className="m-0 text-sm text-text-dimmed">
                  {t('Starting sign-in…')}
                </p>
              )}
              {auth === 'signingOut' && (
                <p className="m-0 text-sm text-text-dimmed">
                  {t('Signing out…')}
                </p>
              )}

              <button
                type="button"
                onClick={handleCancelSignIn}
                className="h-[40px] w-full cursor-pointer rounded-md border border-border-subtle bg-transparent text-sm font-medium text-text-default transition-colors hover:bg-raised"
              >
                {t('Cancel sign-in')}
              </button>
            </div>
          )}

          {isRestoreFailed && (
            <div className="flex flex-col gap-sm">
              <button
                type="button"
                onClick={handleRetryRestore}
                className="h-[40px] w-full cursor-pointer rounded-md border-0 bg-brand text-sm font-medium text-on-primary transition-opacity hover:opacity-90"
              >
                {t('Retry connection')}
              </button>

              <button
                type="button"
                onClick={handleSignOut}
                className="h-[40px] w-full cursor-pointer rounded-md border border-border-subtle bg-transparent text-sm font-medium text-text-default transition-colors hover:bg-raised"
              >
                {t('Sign out')}
              </button>
            </div>
          )}

          <div className="my-[4px] h-px bg-border-subtle" />

          <p className="m-0 text-center text-xs text-text-dimmed">
            {t('Secure sign-in in your default browser.')}
          </p>
        </div>
      </div>
    </div>
  );
}
