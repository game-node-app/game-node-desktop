import React from 'react';
import {
  IconAlertCircle,
  IconAlertTriangle,
  IconBrandSteam,
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

export interface SteamAccountCardProps {
  account: StoreAccountView;
  scanFailed?: boolean;
  onSignOut?: () => void;
}

function formatLastCheck(lastCheckedAt: number, locale: string): string {
  return new Intl.DateTimeFormat(locale, { dateStyle: 'short', timeStyle: 'short' }).format(
    new Date(lastCheckedAt * 1000),
  );
}

export const SteamAccountCard: React.FC<SteamAccountCardProps> = ({
  account,
  scanFailed = false,
  onSignOut,
}) => {
  const { t, locale } = useI18n();

  const status = (() => {
    switch (account.state) {
      case 'notConnected':
        return {
          icon: IconPlugOff,
          label: t('settings.steamStatusNotConnected'),
          pill: settingsPill,
          iconClass: '',
          description: t('settings.steamDescriptionNotConnected'),
        };
      case 'checking':
        return {
          icon: IconLoader2,
          label: t('settings.steamStatusChecking'),
          pill: settingsPillBrand,
          iconClass: 'animate-spin',
          description: t('settings.steamDescriptionChecking'),
        };
      case 'signingIn':
        return {
          icon: IconLoader2,
          label: t('settings.steamStatusSigningIn'),
          pill: settingsPillBrand,
          iconClass: 'animate-spin',
          description: t('settings.steamDescriptionSigningIn'),
        };
      case 'connected':
        return {
          icon: IconCircleCheck,
          label: t('settings.steamStatusConnected'),
          pill: account.persistFailed ? settingsPillWarning : settingsPillSuccess,
          iconClass: '',
          description: t('settings.steamDescriptionConnected'),
        };
      case 'expired':
        return {
          icon: IconClockExclamation,
          label: t('settings.steamStatusExpired'),
          pill: settingsPillWarning,
          iconClass: '',
          description: t('settings.steamDescriptionExpired'),
        };
      case 'unreachable':
        return {
          icon: IconCloudOff,
          label: t('settings.steamStatusUnreachable'),
          pill: settingsPillWarning,
          iconClass: '',
          description: t('settings.steamDescriptionUnreachable'),
        };
      case 'credentialError':
        return {
          icon: IconKey,
          label: t('settings.steamStatusCredentialError'),
          pill: settingsPillWarning,
          iconClass: '',
          description: t('settings.steamDescriptionCredentialError'),
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
    (account.ownedCount > 0 || account.familyCount > 0 || account.installedCount > 0);
  const checkLabel =
    account.lastCheckedAt !== null
      ? t('settings.steamLastCheck', { time: formatLastCheck(account.lastCheckedAt, locale) })
      : account.state === 'notConnected' || account.state === 'signingIn'
        ? null
        : t('settings.steamNeverChecked');

  const storageLabel = (storage: StorageMethod | null): string | null => {
    switch (storage) {
      case 'windowsCredentialManager':
        return t('settings.steamStorageWindows');
      case 'secretService':
        return t('settings.steamStorageSecretService');
      case 'privateFile':
        return t('settings.steamStoragePrivateFile');
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
            {t('settings.steamSignIn')}
          </button>,
        );
      }
      break;
    case 'signingIn':
      if (account.canFocusSignIn) {
        actions.push(
          <button key="focus" type="button" className={settingsButtonPrimary} onClick={focusSignIn}>
            {t('settings.steamBringToFront')}
          </button>,
        );
      }
      if (account.canCancelSignIn) {
        actions.push(
          <button key="cancel" type="button" className={settingsButtonSecondary} onClick={cancelSignIn}>
            {t('settings.steamCancel')}
          </button>,
        );
      }
      break;
    case 'connected':
      if (account.canCheck) {
        actions.push(
          <button key="check" type="button" className={settingsButtonPrimary} onClick={check}>
            {t('settings.steamSyncNow')}
          </button>,
        );
      }
      if (account.canSignOut) {
        actions.push(
          <button key="sign-out" type="button" className={settingsButtonSecondary} onClick={signOut}>
            {t('settings.steamSignOut')}
          </button>,
        );
      }
      break;
    case 'expired':
      if (account.canSignIn) {
        actions.push(
          <button key="sign-in-again" type="button" className={settingsButtonPrimary} onClick={signIn}>
            {t('settings.steamSignInAgain')}
          </button>,
        );
      }
      if (account.canSignOut) {
        actions.push(
          <button key="sign-out" type="button" className={settingsButtonSecondary} onClick={signOut}>
            {t('settings.steamSignOut')}
          </button>,
        );
      }
      break;
    case 'unreachable':
      if (account.canCheck) {
        actions.push(
          <button key="try-again" type="button" className={settingsButtonPrimary} onClick={check}>
            {t('settings.steamTryAgain')}
          </button>,
        );
      }
      if (account.canSignOut) {
        actions.push(
          <button key="sign-out" type="button" className={settingsButtonSecondary} onClick={signOut}>
            {t('settings.steamSignOut')}
          </button>,
        );
      }
      break;
    case 'credentialError':
      if (account.canCheck) {
        actions.push(
          <button key="retry" type="button" className={settingsButtonPrimary} onClick={check}>
            {t('settings.steamRetry')}
          </button>,
        );
      }
      if (account.canSignIn) {
        actions.push(
          <button key="sign-in-again" type="button" className={settingsButtonSecondary} onClick={signIn}>
            {t('settings.steamSignInAgain')}
          </button>,
        );
      }
      if (account.canRemoveCredential) {
        actions.push(
          <button key="remove-credential" type="button" className={settingsButtonGhost} onClick={signOut}>
            {t('settings.steamRemoveSavedSignIn')}
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
      aria-labelledby="steam-account-heading"
      aria-describedby="steam-account-description"
      aria-busy={account.state === 'checking'}
    >
      <div className="flex items-start justify-between gap-md">
        <div className="flex min-w-0 items-start gap-compact">
          <IconBrandSteam size={24} stroke={1.75} className="mt-[2px] shrink-0 text-white" aria-hidden="true" />
          <div className="flex min-w-0 flex-col gap-[2px]">
            <h3 id="steam-account-heading" className={settingsSectionTitle}>
              {t('settings.steamAccount')}
            </h3>
            {identity && (
              <span className={settingsRowDescription}>
                {t('settings.steamSignedInAs', { account: identity })}
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

      <p id="steam-account-description" className={settingsRowDescription}>
        {status.description}
      </p>

      {showCounts && (
        <dl className="flex flex-wrap gap-compact" aria-label={t('settings.steamCountsLabel')}>
          <div className={settingsStat}>
            <dt className={settingsRowDescription}>{t('settings.steamOwned')}</dt>
            <dd className="text-md font-semibold text-white">{account.ownedCount}</dd>
          </div>
          <div className={settingsStat}>
            <dt className={settingsRowDescription}>{t('settings.steamShared')}</dt>
            <dd className="text-md font-semibold text-white">{account.familyCount}</dd>
          </div>
          <div className={settingsStat}>
            <dt className={settingsRowDescription}>{t('settings.steamInstalled')}</dt>
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

      {account.familyError && (
        <div className={settingsInfoBoxWarning} role="status">
          <IconUsers size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.steamFamilyWarning')}</span>
        </div>
      )}

      {account.catalogError && account.state === 'connected' && (
        <div className={settingsInfoBoxWarning} role="status">
          <IconAlertCircle size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.steamCatalogWarning')}</span>
        </div>
      )}

      {account.persistFailed && (
        <div className={settingsInfoBoxWarning} role="alert">
          <IconAlertTriangle size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.steamPersistFailedWarning')}</span>
        </div>
      )}

      {account.storage === 'privateFile' && (
        <div className={settingsInfoBoxWarning} role="note">
          <IconLock size={14} stroke={1.75} aria-hidden="true" />
          <span>{t('settings.steamPrivateFileWarning')}</span>
        </div>
      )}

      <div className={settingsInfoBox} role="note">
        <IconShield size={14} stroke={1.75} aria-hidden="true" />
        <span>{t('settings.steamNeverSeesPassword')}</span>
      </div>

      {actions.length > 0 && (
        <div className="flex flex-wrap items-center gap-compact">{actions}</div>
      )}
    </section>
  );
};
