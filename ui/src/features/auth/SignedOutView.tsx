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
      className={className}
      data-testid="signed-out-view"
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        minHeight: '100%',
        width: '100%',
        padding: '32px',
        boxSizing: 'border-box',
        backgroundColor: 'var(--color-background, #161616)',
        color: 'var(--color-text-default, #D9D9D9)',
      }}
    >
      <div
        style={{
          width: '100%',
          maxWidth: '440px',
          display: 'flex',
          flexDirection: 'column',
          gap: '24px',
        }}
      >
        <NoticeBanner notice={notice} />

        <div style={{ textAlign: 'center' }}>
          <h1
            style={{
              fontSize: '24px',
              fontWeight: 600,
              margin: '0 0 8px 0',
              color: 'var(--color-text-default, #D9D9D9)',
            }}
          >
            {t('Welcome to GameNode')}
          </h1>
          <p
            style={{
              fontSize: '14px',
              margin: 0,
              color: 'var(--color-text-dimmed, #808080)',
            }}
          >
            {t('Your next session starts here.')}
          </p>
        </div>

        <div
          style={{
            backgroundColor: 'var(--color-surface, #262525)',
            border: '1px solid var(--color-border-subtle, #2A2A2A)',
            borderRadius: 'var(--radius-lg, 16px)',
            padding: '32px 24px',
            display: 'flex',
            flexDirection: 'column',
            gap: '20px',
            boxShadow: 'var(--shadow-modal, 0 32px 80px rgba(0,0,0,0.55))',
          }}
        >
          <div style={{ textAlign: 'center' }}>
            <h2
              style={{
                fontSize: '18px',
                fontWeight: 500,
                margin: '0 0 6px 0',
                color: 'var(--color-text-default, #D9D9D9)',
              }}
            >
              {t('Make yourself at home.')}
            </h2>
            <p
              style={{
                fontSize: '13px',
                margin: 0,
                color: 'var(--color-text-dimmed, #808080)',
                lineHeight: 1.4,
              }}
            >
              {t('Sign in with your GameNode account in your browser.')}
            </p>
          </div>

          {!isBusy && !isRestoreFailed && (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: '12px',
              }}
            >
              <button
                type="button"
                onClick={() => handleStartSignIn('google')}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  gap: '10px',
                  height: '40px',
                  backgroundColor: 'var(--color-primary, #F15025)',
                  color: 'var(--color-on-primary, #EDEDED)',
                  border: 'none',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '14px',
                  fontWeight: 500,
                  cursor: 'pointer',
                  transition: 'opacity 0.2s',
                }}
              >
                <span>{t('Continue with Google')}</span>
              </button>

              <button
                type="button"
                onClick={() => handleStartSignIn('discord')}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  gap: '10px',
                  height: '40px',
                  backgroundColor: 'transparent',
                  color: 'var(--color-text-default, #D9D9D9)',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '14px',
                  fontWeight: 500,
                  cursor: 'pointer',
                  transition: 'background-color 0.2s',
                }}
              >
                <span>{t('Continue with Discord')}</span>
              </button>

              <button
                type="button"
                onClick={() => handleStartSignIn('twitter')}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'center',
                  gap: '10px',
                  height: '40px',
                  backgroundColor: 'transparent',
                  color: 'var(--color-text-default, #D9D9D9)',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '14px',
                  fontWeight: 500,
                  cursor: 'pointer',
                  transition: 'background-color 0.2s',
                }}
              >
                <span>{t('Continue with Twitter')}</span>
              </button>
            </div>
          )}

          {isBusy && (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: '16px',
                alignItems: 'center',
                textAlign: 'center',
              }}
            >
              {isPending && (
                <p
                  style={{
                    fontSize: '14px',
                    color: 'var(--color-text-default, #D9D9D9)',
                    margin: 0,
                  }}
                >
                  {t('Complete sign-in in your browser.')}
                </p>
              )}
              {auth === 'verifying' && (
                <p
                  style={{
                    fontSize: '14px',
                    color: 'var(--color-text-dimmed, #808080)',
                    margin: 0,
                  }}
                >
                  {t('Verifying sign-in…')}
                </p>
              )}
              {auth === 'starting' && (
                <p
                  style={{
                    fontSize: '14px',
                    color: 'var(--color-text-dimmed, #808080)',
                    margin: 0,
                  }}
                >
                  {t('Starting sign-in…')}
                </p>
              )}
              {auth === 'signingOut' && (
                <p
                  style={{
                    fontSize: '14px',
                    color: 'var(--color-text-dimmed, #808080)',
                    margin: 0,
                  }}
                >
                  {t('Signing out…')}
                </p>
              )}

              <button
                type="button"
                onClick={handleCancelSignIn}
                style={{
                  width: '100%',
                  height: '40px',
                  backgroundColor: 'transparent',
                  color: 'var(--color-text-default, #D9D9D9)',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '14px',
                  fontWeight: 500,
                  cursor: 'pointer',
                }}
              >
                {t('Cancel sign-in')}
              </button>
            </div>
          )}

          {isRestoreFailed && (
            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: '12px',
              }}
            >
              <button
                type="button"
                onClick={handleRetryRestore}
                style={{
                  width: '100%',
                  height: '40px',
                  backgroundColor: 'var(--color-primary, #F15025)',
                  color: 'var(--color-on-primary, #EDEDED)',
                  border: 'none',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '14px',
                  fontWeight: 500,
                  cursor: 'pointer',
                }}
              >
                {t('Retry connection')}
              </button>

              <button
                type="button"
                onClick={handleSignOut}
                style={{
                  width: '100%',
                  height: '40px',
                  backgroundColor: 'transparent',
                  color: 'var(--color-text-default, #D9D9D9)',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '14px',
                  fontWeight: 500,
                  cursor: 'pointer',
                }}
              >
                {t('Sign out')}
              </button>
            </div>
          )}

          <div
            style={{
              height: '1px',
              backgroundColor: 'var(--color-border-subtle, #2A2A2A)',
              margin: '4px 0',
            }}
          />

          <p
            style={{
              fontSize: '12px',
              color: 'var(--color-text-dimmed, #808080)',
              textAlign: 'center',
              margin: 0,
            }}
          >
            {t('Secure sign-in in your default browser.')}
          </p>
        </div>
      </div>
    </div>
  );
}
