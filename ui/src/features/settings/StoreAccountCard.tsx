import React from 'react';
import {
  IconAlertTriangle,
  IconBrandSteam,
  IconCircleCheck,
  IconCircleX,
  IconDeviceGamepad2,
  IconDots,
  IconExternalLink,
  IconLoader2,
  IconLogout,
  IconRefresh,
  IconTrash,
  IconX,
  type TablerIcon,
} from '@tabler/icons-react';
import { Menu } from '@mantine/core';
import {
  commands,
  type StoreAccountView,
} from '../../api';
import {
  Button,
  IconButton,
  MenuSurface,
  Pill,
  Switch,
} from '../../components/ui';
import { formatRelativeTime } from '../shell/StatusBar';
import { useI18n } from '../../i18n';
import {
  IconBox,
  SettingsRow,
  StatTile,
} from './components';

/** Provider id → Tabler icon, provider-neutral panels use this map (D10). */
export function providerIcon(providerId: string): TablerIcon {
  return providerId === 'steam' ? IconBrandSteam : IconDeviceGamepad2;
}

export interface StoreAccountCardProps {
  account: StoreAccountView;
  /** The provider's display name, for the card's provider-neutral copy. */
  providerName: string;
  /** Whether shared-only games of this provider appear in the library. */
  includeShared?: boolean;
  onIncludeSharedChange?: (checked: boolean) => void;
  onSignOut?: () => void;
}

interface CardAction {
  key: string;
  label: string;
  icon: TablerIcon;
  run: () => void;
}

type PillTone = 'neutral' | 'brand' | 'success' | 'warning' | 'danger';

/**
 * One store account card (S2, D10): provider-neutral identity, state pill,
 * primary action plus a "Mais ações" menu, the three library counts, the
 * shared-games switch and the remove-saved-sign-in row. Attention states keep
 * their warning/danger border and a text explanation, never color alone.
 */
export const StoreAccountCard: React.FC<StoreAccountCardProps> = ({
  account,
  providerName,
  includeShared = true,
  onIncludeSharedChange,
  onSignOut,
}) => {
  const { t } = useI18n();
  const params = { provider: providerName };
  const headingId = `store-account-heading-${account.provider}`;
  const descriptionId = `store-account-description-${account.provider}`;
  const ProviderIcon = providerIcon(account.provider);
  const now = Math.floor(Date.now() / 1000);

  const statePill: {
    tone: PillTone;
    label: string;
    icon?: TablerIcon;
    spinning?: boolean;
  } = (() => {
    switch (account.state) {
      case 'notConnected':
        return {
          tone: 'neutral',
          label: t('settings.stores.state.notConnected'),
          icon: IconCircleX,
        };
      case 'checking':
        return {
          tone: 'brand',
          label: t('settings.stores.state.checking'),
          spinning: true,
        };
      case 'signingIn':
        return {
          tone: 'brand',
          label: t('settings.stores.state.signingIn'),
          spinning: true,
        };
      case 'connected':
        return {
          tone: account.persistFailed ? 'warning' : 'success',
          label: t('settings.stores.state.connected'),
          icon: account.persistFailed ? IconAlertTriangle : IconCircleCheck,
        };
      case 'expired':
        return {
          tone: 'warning',
          label: t('settings.stores.state.expired'),
          icon: IconAlertTriangle,
        };
      case 'unreachable':
        return {
          tone: 'warning',
          label: t('settings.storeStatusUnreachable', params),
          icon: IconAlertTriangle,
        };
      case 'credentialError':
        return {
          tone: 'danger',
          label: t('settings.stores.state.credentialError'),
          icon: IconAlertTriangle,
        };
      default:
        return { tone: 'warning', label: String(account.state) };
    }
  })();

  const identity = account.accountName ?? account.accountId;
  const lastSyncAt = account.lastSyncedAt ?? account.lastCheckedAt;
  const statusLine = (() => {
    switch (account.state) {
      case 'connected':
        if (identity && lastSyncAt !== null) {
          return t('settings.stores.accountSynced', {
            account: identity,
            relative: formatRelativeTime(lastSyncAt, now, t),
          });
        }
        if (identity) {
          return t('settings.storeSignedInAs', { account: identity });
        }
        return t('settings.storeDescriptionConnected', params);
      case 'expired':
        return t('settings.stores.expiredLibrary', { n: account.ownedCount });
      case 'notConnected':
        return t('settings.storeDescriptionNotConnected', params);
      case 'checking':
        return t('settings.storeDescriptionChecking', params);
      case 'signingIn':
        return t('settings.storeDescriptionSigningIn', params);
      case 'unreachable':
        return t('settings.storeDescriptionUnreachable', params);
      case 'credentialError':
        return t('settings.storeDescriptionCredentialError', params);
      default:
        return '';
    }
  })();

  const showShared = account.sharedLabel !== null;
  const showStats = account.state !== 'signingIn';
  const sharedCaption = showShared
    ? t('settings.stores.stat.sharedWith', {
        label: account.sharedLabel ?? '',
      })
    : '';
  const showRemoveSavedSignIn =
    account.state === 'expired' || account.state === 'credentialError';
  const canRemoveSavedSignIn =
    account.state === 'credentialError'
      ? account.canRemoveCredential
      : account.canSignOut;

  const signIn = () => {
    void commands.providerSignIn(account.provider);
  };
  const focusSignIn = () => {
    void commands.providerFocusSignIn(account.provider);
  };
  const cancelSignIn = () => {
    void commands.providerCancelSignIn(account.provider);
  };
  const check = () => {
    void commands.providerCheck(account.provider);
  };
  const signOut = () => {
    void commands.providerSignOut(account.provider);
    onSignOut?.();
  };

  const signInAction: CardAction = {
    key: 'sign-in',
    label: t('settings.stores.action.signIn'),
    icon: IconExternalLink,
    run: signIn,
  };
  const signInAgainAction: CardAction = {
    key: 'sign-in-again',
    label: t('settings.stores.action.signInAgain'),
    icon: IconExternalLink,
    run: signIn,
  };
  const syncNowAction: CardAction = {
    key: 'sync-now',
    label: t('settings.stores.action.syncNow'),
    icon: IconRefresh,
    run: check,
  };
  const tryAgainAction: CardAction = {
    key: 'try-again',
    label: t('settings.stores.action.tryAgain'),
    icon: IconRefresh,
    run: check,
  };
  const showWindowAction: CardAction = {
    key: 'show-window',
    label: t('settings.stores.action.showSignInWindow'),
    icon: IconExternalLink,
    run: focusSignIn,
  };
  const cancelAction: CardAction = {
    key: 'cancel-sign-in',
    label: t('settings.stores.action.cancelSignIn'),
    icon: IconX,
    run: cancelSignIn,
  };
  const signOutAction: CardAction = {
    key: 'sign-out',
    label: t('settings.stores.action.signOut'),
    icon: IconLogout,
    run: signOut,
  };
  const removeSavedSignInAction: CardAction = {
    key: 'remove-saved-sign-in',
    label: t('settings.stores.removeSavedSignIn'),
    icon: IconTrash,
    run: signOut,
  };

  const actions: CardAction[] = [];
  switch (account.state) {
    case 'notConnected':
      if (account.canSignIn) actions.push(signInAction);
      break;
    case 'signingIn':
      if (account.canFocusSignIn) actions.push(showWindowAction);
      if (account.canCancelSignIn) actions.push(cancelAction);
      break;
    case 'connected':
      if (account.canCheck) actions.push(syncNowAction);
      if (account.canSignOut) actions.push(signOutAction);
      break;
    case 'expired':
      if (account.canSignIn) actions.push(signInAgainAction);
      if (account.canSignOut) actions.push(removeSavedSignInAction);
      break;
    case 'unreachable':
      if (account.canCheck) actions.push(tryAgainAction);
      if (account.canSignOut) actions.push(signOutAction);
      break;
    case 'credentialError':
      if (account.canCheck) actions.push(tryAgainAction);
      if (account.canSignIn) actions.push(signInAgainAction);
      if (canRemoveSavedSignIn) actions.push(removeSavedSignInAction);
      break;
    default:
      break;
  }
  const [primaryAction, ...menuActions] = actions;

  const tone =
    account.state === 'credentialError'
      ? 'border-danger'
      : account.state === 'expired' ||
          account.state === 'unreachable' ||
          account.persistFailed
        ? 'border-warning'
        : 'border-border-subtle';

  return (
    <section
      data-provider={account.provider}
      aria-labelledby={headingId}
      aria-describedby={descriptionId}
      aria-busy={
        account.state === 'checking' || account.state === 'signingIn'
          ? true
          : undefined
      }
      className={`flex flex-col overflow-hidden rounded-card border bg-secondary ${tone}`}
    >
      <div className="flex flex-wrap items-center gap-row p-md">
        <IconBox icon={ProviderIcon} />
        <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
          <h3
            id={headingId}
            className="truncate text-md font-semibold text-text-primary"
          >
            {providerName}
          </h3>
          <p
            id={descriptionId}
            className="text-xs leading-note text-text-dimmed"
          >
            {statusLine}
          </p>
        </div>
        <span role="status" className="inline-flex items-center">
          <Pill tone={statePill.tone} icon={statePill.spinning ? undefined : statePill.icon}>
            {statePill.spinning ? (
              <IconLoader2
                size={12}
                stroke={2}
                aria-hidden="true"
                className="animate-spin motion-reduce:animate-none"
              />
            ) : null}
            {statePill.label}
          </Pill>
        </span>
        {primaryAction ? (
          <Button
            size="sm"
            variant={
              account.state === 'notConnected' ||
              account.state === 'expired' ||
              account.state === 'credentialError'
                ? 'primary'
                : 'secondary'
            }
            leftIcon={primaryAction.icon}
            onClick={primaryAction.run}
            aria-label={primaryAction.label}
          >
            {primaryAction.label}
          </Button>
        ) : null}
        {menuActions.length > 0 ? (
          <MenuSurface position="bottom-end" width={220} withinPortal>
            <Menu.Target>
              <IconButton
                icon={IconDots}
                label={t('settings.stores.action.more')}
                variant="outlined"
                size="sm"
                className="!h-[30px] !w-[30px]"
              />
            </Menu.Target>
            <Menu.Dropdown>
              {menuActions.map((action) => {
                const ActionIcon = action.icon;
                return (
                  <Menu.Item
                    key={action.key}
                    leftSection={<ActionIcon size={16} stroke={2} aria-hidden="true" />}
                    onClick={action.run}
                  >
                    {action.label}
                  </Menu.Item>
                );
              })}
            </Menu.Dropdown>
          </MenuSurface>
        ) : null}
      </div>

      {showStats ? (
        <div
          role="group"
          aria-label={t('settings.storeCountsLabel', params)}
          className="flex gap-compact px-md pb-row"
        >
          <StatTile
            value={account.ownedCount}
            caption={t('settings.stores.stat.inLibrary')}
          />
          {showShared ? (
            <StatTile value={account.sharedCount} caption={sharedCaption} />
          ) : null}
          <StatTile
            value={account.installedCount}
            caption={t('settings.stores.stat.installedHere')}
          />
        </div>
      ) : null}

      {showShared && onIncludeSharedChange ? (
        <SettingsRow
          compact
          label={t('settings.stores.includeShared')}
          help={t('settings.stores.includeSharedHelp', {
            sharedLabel: account.sharedLabel ?? '',
          })}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              checked={includeShared}
              onChange={(event) =>
                onIncludeSharedChange(event.currentTarget.checked)
              }
            />
          )}
        />
      ) : null}

      {showRemoveSavedSignIn ? (
        <SettingsRow
          compact
          label={t('settings.stores.removeSavedSignIn')}
          help={t('settings.stores.removeSavedSignInHelp')}
          control={
            <Button
              size="sm"
              variant="ghost"
              leftIcon={IconTrash}
              onClick={signOut}
              disabled={!canRemoveSavedSignIn}
            >
              {t('settings.common.remove')}
            </Button>
          }
        />
      ) : null}

      {account.state === 'expired' ? (
        <div className="flex items-start gap-compact border-t border-border-subtle bg-warning-subtle px-md py-sm text-xs leading-note text-text-secondary">
          <IconAlertTriangle
            size={14}
            stroke={2}
            aria-hidden="true"
            className="mt-[1px] shrink-0 text-warning"
          />
          <span>{t('settings.stores.expiredHelp')}</span>
        </div>
      ) : null}

      {account.persistFailed ? (
        <div
          role="alert"
          className="flex items-start gap-compact border-t border-border-subtle bg-warning-subtle px-md py-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={14}
            stroke={2}
            aria-hidden="true"
            className="mt-[1px] shrink-0 text-warning"
          />
          <span>{t('settings.storePersistFailedWarning', params)}</span>
        </div>
      ) : null}

      {showShared && account.sharedError ? (
        <div
          role="status"
          className="flex items-start gap-compact border-t border-border-subtle bg-warning-subtle px-md py-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={14}
            stroke={2}
            aria-hidden="true"
            className="mt-[1px] shrink-0 text-warning"
          />
          <span>
            {t('settings.storeSharedWarning', {
              label: account.sharedLabel ?? '',
            })}
          </span>
        </div>
      ) : null}

      {account.catalogError && account.state === 'connected' ? (
        <div
          role="status"
          className="flex items-start gap-compact border-t border-border-subtle bg-warning-subtle px-md py-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={14}
            stroke={2}
            aria-hidden="true"
            className="mt-[1px] shrink-0 text-warning"
          />
          <span>{t('settings.storeCatalogWarning', params)}</span>
        </div>
      ) : null}
    </section>
  );
};
