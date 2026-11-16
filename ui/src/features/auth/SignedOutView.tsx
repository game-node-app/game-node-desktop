import { useQuery } from '@tanstack/react-query';
import {
  IconAlertTriangle,
  IconExternalLink,
  IconLoader2,
  IconLock,
} from '@tabler/icons-react';
import { commands } from '../../api/commands';
import { signInCoverWallQueryOptions } from '../../api/queries';
import type { AuthView, Notice, Provider } from '../../api/types';
import { useAppInfo, useAuth } from '../../api/useSnapshot';
import { Button } from '../../components/ui/Button';
import { Banner } from '../../components/ui/Banner';
import { cx } from '../../components/ui/cx';
import { ConfirmDialog } from '../../components/dialog';
import { useI18n } from '../../i18n';
import wordmark from '../../assets/icons/gamenode-logo.svg';
import { CoverMural } from './CoverMural';
import { NoticeBanner } from './NoticeBanner';
import { ProviderButton } from './ProviderButton';

const PROVIDERS: Provider[] = ['google', 'discord', 'twitter'];

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
  const appInfo = useAppInfo();
  const covers = useQuery(signInCoverWallQueryOptions);
  const { t } = useI18n();

  const auth = propAuth !== undefined ? propAuth : authState.auth;
  const notice = propNotice !== undefined ? propNotice : authState.notice;

  const pending =
    typeof auth === 'object' && auth !== null && 'pending' in auth
      ? auth.pending
      : null;
  const isRestoreFailed = auth === 'restoreFailed';
  const isVerifying = auth === 'verifying';
  const isStarting = auth === 'starting';
  const isSigningOut = auth === 'signingOut';
  const showProviders = auth === 'signedOut' || pending !== null;

  const handleStartSignIn = (provider: Provider) => {
    commands.dispatchAuth({ startSignIn: provider });
  };

  const handleCancelSignIn = () => {
    commands.dispatchAuth('cancelSignIn');
  };

  const handleReopenBrowser = () => {
    commands.dispatchAuth('reopenBrowser');
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
      className={cx(
        'flex min-h-full w-full bg-body text-text-primary',
        className,
      )}
    >
      <section
        data-testid="sign-in-art"
        className="relative hidden min-w-0 basis-[53%] flex-col justify-end overflow-hidden bg-chrome p-12 max-w-[760px] min-[860px]:flex"
      >
        <CoverMural files={covers.data ?? []} />
        <div className="relative z-10 flex max-w-[520px] flex-col gap-compact">
          <p className="m-0 text-hero font-semibold text-text-primary">
            {t('auth.storesHeadline')}
          </p>
          <p className="m-0 text-title font-normal leading-hero text-text-secondary">
            {t('auth.storesBody')}
          </p>
        </div>
      </section>

      <section className="relative flex min-w-0 flex-1 flex-col items-center justify-center px-xl">
        <div className="flex w-full max-w-[400px] flex-col gap-lg">
          <img
            src={wordmark}
            width={83}
            height={32}
            alt="GameNode"
            draggable={false}
            className="block"
          />

          <div className="flex flex-col gap-1.5">
            <h1 className="m-0 text-display font-semibold text-text-primary">
              {t('Welcome to GameNode')}
            </h1>
            <p className="m-0 text-sm text-text-secondary">
              {t('Sign in with your GameNode account in your browser.')}
            </p>
          </div>

          {notice != null && !isRestoreFailed ? (
            <NoticeBanner notice={notice} />
          ) : null}

          {isStarting ? (
            <Banner
              tone="neutral"
              icon={IconLoader2}
              title={t('auth.starting')}
            />
          ) : null}

          {isVerifying ? (
            <Banner
              tone="neutral"
              icon={IconLoader2}
              title={t('auth.verifying')}
              action={{
                label: t('Cancel sign-in'),
                onClick: handleCancelSignIn,
              }}
            />
          ) : null}

          {isSigningOut ? (
            <Banner
              tone="neutral"
              icon={IconLoader2}
              title={t('auth.signingOut')}
            />
          ) : null}

          {isRestoreFailed ? (
            <div className="flex flex-col gap-xs">
              <Banner
                tone="warning"
                icon={IconAlertTriangle}
                title={t(
                  'Could not connect. Your stored session is preserved; retry when online.',
                )}
                action={{
                  label: t('Retry connection'),
                  onClick: handleRetryRestore,
                }}
              />
              <Button variant="ghost" fullWidth onClick={handleSignOut}>
                {t('Sign out')}
              </Button>
            </div>
          ) : null}

          {showProviders ? (
            <div className="flex flex-col gap-xs">
              {PROVIDERS.map((provider) => (
                <ProviderButton
                  key={provider}
                  provider={provider}
                  onClick={() => handleStartSignIn(provider)}
                />
              ))}
            </div>
          ) : null}

          <div className="flex items-center gap-compact text-xs text-text-dimmed">
            <IconLock size={14} stroke={2} aria-hidden="true" />
            <span>{t('auth.securityNote')}</span>
          </div>
        </div>

        {appInfo.data?.version ? (
          <p
            data-testid="sign-in-version"
            className="absolute bottom-5 right-6 m-0 text-caption text-text-muted"
          >
            {t('auth.version', { version: appInfo.data.version })}
          </p>
        ) : null}
      </section>

      {pending ? (
        <ConfirmDialog
          tone="accent"
          icon={IconExternalLink}
          title={t('auth.pendingTitle')}
          description={t('auth.pendingBody')}
          size={420}
          onClose={handleCancelSignIn}
          testId="sign-in-pending"
          actions={
            <>
              <Button
                variant="ghost"
                size="sm"
                leftIcon={IconExternalLink}
                disabled={!pending.canReopen}
                onClick={handleReopenBrowser}
              >
                {t('auth.openAgain')}
              </Button>
              <div className="flex-1" />
              <Button
                variant="secondary"
                size="sm"
                onClick={handleCancelSignIn}
              >
                {t('Cancel sign-in')}
              </Button>
            </>
          }
        >
          <div
            data-testid="sign-in-pending-status"
            role="status"
            className="flex items-center gap-xs rounded-md border border-border-subtle bg-body p-xs text-xs text-text-secondary"
          >
            <IconLoader2
              size={16}
              stroke={2}
              aria-hidden="true"
              className="shrink-0 animate-spin text-brand motion-reduce:animate-none"
            />
            <span>
              {pending.awaitingBrowser
                ? t('auth.pendingWaiting')
                : t('auth.pendingConfirming')}
            </span>
          </div>

          {pending.reopenFailed ? (
            <p
              role="alert"
              data-testid="sign-in-reopen-failed"
              className="m-0 flex items-start gap-compact text-xs text-danger"
            >
              <IconAlertTriangle
                size={14}
                stroke={2}
                aria-hidden="true"
                className="mt-[1px] shrink-0"
              />
              <span>{t('auth.reopenFailed')}</span>
            </p>
          ) : null}
        </ConfirmDialog>
      ) : null}
    </div>
  );
}
