import React from 'react';
import {
  IconAlertCircle,
  IconAlertTriangle,
  IconBuildingStore,
  IconCircleCheck,
  IconClockExclamation,
  IconCloudOff,
  IconKey,
  IconLoader2,
  IconLock,
  IconPlugOff,
  IconShield,
  IconUsers,
} from '@tabler/icons-react';
import { commands, type StorageMethod, type StoreAccountView } from '../../api';
import { useI18n } from '../../i18n';
import {
  settingsButtonGhost,
  settingsButtonPrimary,
  settingsButtonSecondary,
  settingsCardRoomy,
  settingsInfoBox,
  settingsInfoBoxWarning,
  settingsPill,
  settingsPillBrand,
  settingsPillSuccess,
  settingsPillWarning,
  settingsRowDescription,
  settingsSectionTitle,
  settingsStat,
} from './settingsClasses';

export interface StoreAccountCardProps {
  account: StoreAccountView;
  /** The provider's display name, for the card's provider-neutral copy. */
  providerName: string;
  scanFailed?: boolean;
  onSignOut?: () => void;
}

function formatLastCheck(lastCheckedAt: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'short', timeStyle: 'short' }).format(
    new Date(lastCheckedAt * 1000),
  );
}

export const StoreAccountCard: React.FC<StoreAccountCardProps> = ({
  account,
  providerName,
  scanFailed = false,
  onSignOut,
}) => {
  const { t, locale } = useI18n();
  const params = { provider: providerName };
  const headingId = `store-account-heading-${account.provider}`;
  const descriptionId = `store-account-description-${account.provider}`;

  const status = (() => {
    switch (account.state) {
      case 'notConnected':
        return {
          icon: IconPlugOff,
          label: t('settings.storeStatusNotConnected'),
          pill: settingsPill,
          iconClass: '',
          description: t('settings.storeDescriptionNotConnected', params),
        };
      case 'checking':
        return {
          icon: IconLoader2,
          label: t('settings.storeStatusChecking'),
          pill: settingsPillBrand,
          iconClass: 'animate-spin',
          description: t('settings.storeDescriptionChecking', params),
        };
      case 'signingIn':
        return {
          icon: IconLoader2,
          label: t('settings.storeStatusSigningIn'),
          pill: settingsPillBrand,
          iconClass: 'animate-spin',
          description: t('settings.storeDescriptionSigningIn', params),
        };
      case 'connected':
        return {
          icon: IconCircleCheck,
          label: t('settings.storeStatusConnected'),
          pill: account.persistFailed ? settingsPillWarning : settingsPillSuccess,
          iconClass: '',
          description: t('settings.storeDescriptionConnected', params),
        };
      case 'expired':
        return {
          icon: IconClockExclamation,
          label: t('settings.storeStatusExpired'),
          pill: settingsPillWarning,
          iconClass: '',
          description: t('settings.storeDescriptionExpired', params),
        };
      case 'unreachable':
        return {
          icon: IconCloudOff,
          label: t('settings.storeStatusUnreachable', params),
          pill: settingsPillWarning,
          iconClass: '',
          description: t('settings.storeDescriptionUnreachable', params),
        };
      case 'credentialError':
        return {
          icon: IconKey,
          label: t('settings.storeStatusCredentialError'),
          pill: settingsPillWarning,
          iconClass: '',
          description: t('settings.storeDescriptionCredentialError', params),
        };
      default:
        return {
          icon: IconAlertCircle,
          label: String(account.state),
          pill: settingsPillWarning,
          iconClass: '',
          description: '',
        };
    }
  })();
  const StatusIcon = status.icon;

  const identity = account.accountName ?? account.accountId;
  const showCounts =
    account.state !== 'signingIn' &&
    (account.ownedCount > 0 || account.sharedCount > 0 || account.installedCount > 0);
  // Providers that declare no shared access never report shared counts or a shared outcome.
  const showShared = account.sharedLabel !== null;
  const sharedParams = { label: account.sharedLabel ?? '' };
  const checkLabel =
    account.lastCheckedAt !== null
      ? t('settings.storeLastCheck', { time: formatLastCheck(account.lastCheckedAt, locale) })
      : account.state === 'notConnected' || account.state === 'signingIn'
        ? null
        : t('settings.storeNeverChecked');

  const storageLabel = (storage: StorageMethod | null): string | null => {
    switch (storage) {
      case 'windowsCredentialManager':
        return t('settings.storeStorageWindows');
      case 'secretService':
        return t('settings.storeStorageSecretService');
      case 'privateFile':
        return t('settings.storeStoragePrivateFile');
      default:
        return null;
    }
  };

  const signIn = () => { void commands.providerSignIn(account.provider); };
  const focusSignIn = () => { void commands.providerFocusSignIn(account.provider); };
  const cancelSignIn = () => { void commands.providerCancelSignIn(account.provider); };
  const check = () => { void commands.providerCheck(account.provider); };
  const signOut = () => {
    void commands.providerSignOut(account.provider);
    onSignOut?.();
  };

  const actions: React.ReactNode[] = [];
  switch (account.state) {
    case 'notConnected':
      if (account.canSignIn) {
        actions.push(
          <button key="sign-in" type="button" className={settingsButtonPrimary} onClick={signIn}>
            {t('settings.storeSignIn', params)}
          </button>,
        );
      }
      break;
    case 'signingIn':
      if (account.canFocusSignIn) {
        actions.push(
          <button key="focus" type="button" className={settingsButtonPrimary} onClick={focusSignIn}>
            {t('settings.storeBringToFront')}
          </button>,
        );
      }
      if (account.canCancelSignIn) {
        actions.push(
          <button key="cancel" type="button" className={settingsButtonSecondary} onClick={cancelSignIn}>
            {t('settings.storeCancel')}
          </button>,
        );
      }
      break;
    case 'connected':
      if (account.canCheck) {
        actions.push(
          <button key="check" type="button" className={settingsButtonPrimary} onClick={check}>
            {t('settings.storeSyncNow')}
          </button>,
        );
      }
      if (account.canSignOut) {
        actions.push(
          <button key="sign-out" type="button" className={settingsButtonSecondary} onClick={signOut}>
            {t('settings.storeSignOut')}
          </button>,
        );
      }
      break;
    case 'expired':
      if (account.canSignIn) {
        actions.push(
          <button key="sign-in-again" type="button" className={settingsButtonPrimary} onClick={signIn}>
            {t('settings.storeSignInAgain')}
          </button>,
        );
      }
      if (account.canSignOut) {
        actions.push(
          <button key="sign-out" type="button" className={settingsButtonSecondary} onClick={signOut}>
            {t('settings.storeSignOut')}
          </button>,
        );
      }
      break;
    case 'unreachable':
      if (account.canCheck) {
        actions.push(
          <button key="try-again" type="button" className={settingsButtonPrimary} onClick={check}>
            {t('settings.storeTryAgain')}
          </button>,
        );
      }
      if (account.canSignOut) {
        actions.push(
          <button key="sign-out" type="button" className={settingsButtonSecondary} onClick={signOut}>
            {t('settings.storeSignOut')}
          </button>,
        );
      }
      break;
    case 'credentialError':
      if (account.canCheck) {
        actions.push(
          <button key="retry" type="button" className={settingsButtonPrimary} onClick={check}>
            {t('settings.storeRetry')}
          </button>,
        );
      }
      if (account.canSignIn) {
        actions.push(
          <button key="sign-in-again" type="button" className={settingsButtonSecondary} onClick={signIn}>
            {t('settings.storeSignInAgain')}
          </button>,
        );
      }
      if (account.canRemoveCredential) {
        actions.push(
          <button key="remove-credential" type="button" className={settingsButtonGhost} onClick={signOut}>
            {t('settings.storeRemoveSavedSignIn')}
          </button>,
        );
      }
      break;
    default:
      break;
  }

  return (
    <section
      className={settingsCardRoomy}
      aria-labelledby={headingId}
      aria-describedby={descriptionId}
      aria-busy={account.state === 'checking'}
    >
      <div className="flex items-start justify-between gap-md">
        <div className="flex min-w-0 items-start gap-compact">
          <IconBuildingStore size={24} stroke={1.75} className="mt-[2px] shrink-0 text-white" aria-hidden="true" />
          <div className="flex min-w-0 flex-col gap-[2px]">
            <h3 id={headingId} className={settingsSectionTitle}>
              {t('settings.storeAccount', params)}
            </h3>
            {identity && (
              <span className={settingsRowDescription}>
                {t('settings.storeSignedInAs', { account: identity })}
              </span>
            )}
          </div>
        </div>
        <span role="status" className={status.pill}>
          <StatusIcon
            size={14}
            stroke={1.75}
            className={status.iconClass}
            aria-hidden="true"
          />
          <span>{status.label}</span>
        </span>
      </div>

      <p id={descriptionId} className={settingsRowDescription}>
        {status.description}
      </p>

      {showCounts && (
        <dl className="flex flex-wrap gap-compact" aria-label={t('settings.storeCountsLabel', params)}>
          <div className={settingsStat}>
            <dt className={settingsRowDescription}>{t('settings.storeOwned')}</dt>
            <dd className="text-md font-semibold text-white">{account.ownedCount}</dd>
          </div>
          {showShared && (
            <div className={settingsStat}>
              <dt className={settingsRowDescription}>{account.sharedLabel}</dt>
              <dd className="text-md font-semibold text-white">{account.sharedCount}</dd>
            </div>
          )}
          <div className={settingsStat}>
            <dt className={settingsRowDescription}>{t('settings.storeInstalled')}</dt>
            <dd className="text-md font-semibold text-white">{account.installedCount}</dd>
          </div>
        </dl>
      )}

      <div className="flex flex-wrap items-center gap-x-md gap-y-1">
        <span className={settingsRowDescription}>
          {scanFailed
            ? t('settings.lastScanFailed')
            : account.installedCount > 0
              ? `${account.installedCount} ${t('settings.installedGames')}`
              : t('settings.noInstalledGamesFound')}
        </span>
        {checkLabel && <span className={settingsRowDescription}>{checkLabel}</span>}
        {account.storage && (
          <span className={settingsRowDescription}>{storageLabel(account.storage)}</span>
        )}
      </div>

      {showShared && account.sharedError && (
        <div className={settingsInfoBoxWarning} role="status">
          <IconUsers size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.storeSharedWarning', sharedParams)}</span>
        </div>
      )}

      {account.catalogError && account.state === 'connected' && (
        <div className={settingsInfoBoxWarning} role="status">
          <IconAlertCircle size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.storeCatalogWarning', params)}</span>
        </div>
      )}

      {account.persistFailed && (
        <div className={settingsInfoBoxWarning} role="alert">
          <IconAlertTriangle size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.storePersistFailedWarning', params)}</span>
        </div>
      )}

      {account.storage === 'privateFile' && (
        <div className={settingsInfoBoxWarning} role="note">
          <IconLock size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.storePrivateFileWarning', params)}</span>
        </div>
      )}

      <div className={settingsInfoBox} role="note">
        <IconShield size={14} stroke={1.75} aria-hidden="true" />
        <span>{t('settings.storeNeverSeesPassword', params)}</span>
      </div>

      {actions.length > 0 && (
        <div className="flex flex-wrap items-center gap-compact">{actions}</div>
      )}
    </section>
  );
};
