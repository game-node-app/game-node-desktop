import React, { useEffect, useState } from 'react';
import {
  IconAlertTriangle,
  IconCircleCheck,
  IconCircleX,
  IconExternalLink,
  IconLock,
  IconLogout,
  IconRefresh,
  IconUser,
} from '@tabler/icons-react';
import { commands, useAuth, useLibraryView } from '../../../api';
import type { Provider, StorageMethod } from '../../../api';
import {
  Button,
  Pill,
  SegmentedControl,
  Switch,
} from '../../../components/ui';
import { ProviderButton } from '../../auth/ProviderButton';
import { formatRelativeTime } from '../../shell/StatusBar';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import { SettingsCard, SettingsPanel, SettingsRow } from '../components';

const PROVIDERS: Provider[] = ['google', 'discord', 'twitter'];

function storageLabelKey(storage: StorageMethod): string {
  switch (storage) {
    case 'windowsCredentialManager':
      return 'settings.account.storage.windowsVault';
    case 'secretService':
      return 'settings.account.storage.keyring';
    case 'privateFile':
    default:
      return 'settings.account.storage.privateFile';
  }
}

/**
 * Conta e sincronização (S1): identity and session storage, the sync and
 * playtime-source preferences, the last sync summary with Sincronizar agora
 * and the static list of what leaves this PC. Preferences apply immediately
 * with an optimistic value and roll back on a command error (D4, D9).
 */
export const AccountPanel: React.FC = () => {
  const { t } = useI18n();
  const { auth, storage, sync, playtimeSource, playtimeError } = useAuth();
  const library = useLibraryView();

  const isSignedIn =
    typeof auth === 'object' && auth !== null && 'signedIn' in auth;
  const user = isSignedIn ? auth.signedIn.user : null;
  const identity = user?.username ?? user?.email ?? user?.id ?? '';
  const avatarLetter = identity.trim().charAt(0).toUpperCase();

  const [syncOverride, setSyncOverride] = useState<boolean | null>(null);
  const [sourceOverride, setSourceOverride] = useState<
    'store' | 'internal' | null
  >(null);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const syncEnabled = syncOverride ?? sync.enabled;
  const source = sourceOverride ?? playtimeSource;
  const syncRunning = sync.running || syncing;

  useEffect(() => {
    if (syncOverride !== null && sync.enabled === syncOverride) {
      setSyncOverride(null);
    }
  }, [sync.enabled, syncOverride]);

  useEffect(() => {
    if (sourceOverride !== null && playtimeSource === sourceOverride) {
      setSourceOverride(null);
    }
  }, [playtimeSource, sourceOverride]);

  const handleToggleSync = async (enabled: boolean) => {
    setSyncOverride(enabled);
    setError(null);
    try {
      await commands.setSyncEnabled(enabled);
      pushToast('success', t('settings.account.syncWithGamenode'));
    } catch {
      setSyncOverride(null);
      setError(t('settings.account.saveFailed'));
    }
  };

  const handleSelectPlaytimeSource = async (next: 'store' | 'internal') => {
    if (next === source) return;
    setSourceOverride(next);
    setError(null);
    try {
      await commands.setPlaytimeSource(next);
      pushToast('success', t('settings.account.playtimeSource'));
    } catch {
      setSourceOverride(null);
      setError(t('settings.account.saveFailed'));
    }
  };

  const handleSyncNow = async () => {
    setSyncing(true);
    setError(null);
    try {
      await commands.syncNow();
    } catch {
      setError(t('settings.account.syncFailed'));
    } finally {
      setSyncing(false);
    }
  };

  const handleOpenProfile = () => {
    setError(null);
    void commands.openGameNodeLink('profile').catch(() => {
      setError(t('settings.account.openProfileFailed'));
    });
  };

  const handleSignOut = () => {
    void commands.dispatchAuth('signOut');
  };

  const handleStartSignIn = (provider: Provider) => {
    void commands.dispatchAuth({ startSignIn: provider });
  };

  const summary = sync.summary;
  const canSyncNow =
    isSignedIn &&
    syncEnabled &&
    !syncRunning &&
    library?.access === 'verified';
  const now = Math.floor(Date.now() / 1000);
  const relative =
    summary && syncEnabled
      ? formatRelativeTime(summary.completedAt, now, t)
      : null;
  const summaryText = !syncEnabled
    ? t('settings.account.syncOff')
    : summary
      ? summary.held > 0
        ? t('settings.account.lastSyncSummaryPending', {
            games: summary.acceptedGames,
            stores: summary.stores,
            pending: summary.held,
          })
        : t('settings.account.lastSyncSummaryOk', {
            games: summary.acceptedGames,
            stores: summary.stores,
          })
      : t('settings.account.neverSynced');

  return (
    <SettingsPanel
      title={t('settings.nav.accountSync')}
      description={t('settings.account.description')}
    >
      {error ? (
        <div
          role="alert"
          data-testid="settings-account-error"
          className="flex items-start gap-compact rounded-md border border-danger bg-danger-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-danger"
          />
          <span>{error}</span>
        </div>
      ) : null}

      <section
        aria-labelledby="settings-account-identity"
        className="flex flex-col gap-md rounded-card border border-border-subtle bg-secondary p-md"
      >
        <div className="flex flex-wrap items-center gap-row">
          {isSignedIn ? (
            <span
              aria-hidden="true"
              className="flex h-[48px] w-[48px] shrink-0 items-center justify-center rounded-full bg-avatar text-[18px] font-semibold text-text-primary"
            >
              {avatarLetter}
            </span>
          ) : (
            <span className="flex h-[48px] w-[48px] shrink-0 items-center justify-center rounded-full bg-raised text-text-muted">
              <IconUser size={20} stroke={2} aria-hidden="true" />
            </span>
          )}

          <div className="flex min-w-0 flex-1 flex-col gap-[3px]">
            <h3
              id="settings-account-identity"
              className="truncate text-md font-semibold text-text-primary"
            >
              {isSignedIn
                ? identity
                : t('settings.account.noSession')}
            </h3>
            {isSignedIn ? (
              <span className="flex min-w-0 items-center gap-[6px] text-xs text-text-dimmed">
                <IconLock size={12} stroke={2} aria-hidden="true" />
                <span className="truncate">
                  {t(storageLabelKey(storage))}
                </span>
              </span>
            ) : (
              <span className="text-xs leading-note text-text-dimmed">
                {t('settings.account.signInWithGamenode')}
              </span>
            )}
          </div>

          {isSignedIn ? (
            <>
              <Pill tone="success" icon={IconCircleCheck}>
                {t('settings.account.connected')}
              </Pill>
              <Button
                size="sm"
                variant="secondary"
                leftIcon={IconExternalLink}
                onClick={handleOpenProfile}
              >
                {t('settings.account.openProfile')}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                leftIcon={IconLogout}
                onClick={handleSignOut}
              >
                {t('settings.account.signOut')}
              </Button>
            </>
          ) : null}
        </div>

        {!isSignedIn ? (
          <div className="flex flex-col gap-compact">
            {PROVIDERS.map((provider) => (
              <ProviderButton
                key={provider}
                provider={provider}
                onClick={() => handleStartSignIn(provider)}
              />
            ))}
          </div>
        ) : null}
      </section>

      <SettingsCard title={t('settings.account.playtimeSync')}>
        <SettingsRow
          label={t('settings.account.syncWithGamenode')}
          help={t('settings.account.syncHelp')}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              checked={syncEnabled}
              onChange={(event) =>
                void handleToggleSync(event.currentTarget.checked)
              }
            />
          )}
        />
        <SettingsRow
          label={t('settings.account.playtimeSource')}
          help={t('settings.account.playtimeSourceHelp')}
          control={(ids) => (
            <SegmentedControl<'store' | 'internal'>
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              segmentPadding="regular"
              value={source}
              onChange={(next) => void handleSelectPlaytimeSource(next)}
              data={[
                {
                  value: 'store',
                  label: t('settings.account.playtimeSourceStore'),
                },
                {
                  value: 'internal',
                  label: t('settings.account.playtimeSourceInternal'),
                },
              ]}
            />
          )}
        />
        <SettingsRow
          label={t('settings.account.lastSync')}
          help={summaryText}
          control={
            <div className="flex items-center gap-compact">
              {relative ? (
                <span className="text-xs text-text-dimmed">{relative}</span>
              ) : null}
              {isSignedIn ? (
                <Button
                  size="sm"
                  variant="secondary"
                  leftIcon={IconRefresh}
                  loading={syncRunning}
                  disabled={!canSyncNow}
                  onClick={() => void handleSyncNow()}
                >
                  {syncRunning
                    ? t('settings.account.syncing')
                    : t('settings.account.syncNow')}
                </Button>
              ) : null}
            </div>
          }
        />
      </SettingsCard>

      {playtimeError ? (
        <div
          role="alert"
          className="flex items-start gap-compact rounded-md border border-warning bg-warning-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-warning"
          />
          <span>{t('Could not sync playtime. Your local data is preserved.')}</span>
        </div>
      ) : null}

      <SettingsCard title={t('settings.account.whatIsSynced')}>
        <div className="grid grid-cols-1 gap-md border-t border-border-subtle p-md sm:grid-cols-2">
          <div className="flex min-w-0 flex-col gap-compact">
            {[
              t('settings.account.sent.playtime'),
              t('settings.account.sent.collections'),
            ].map((item) => (
              <div key={item} className="flex items-center gap-compact">
                <IconCircleCheck
                  size={14}
                  stroke={2}
                  aria-hidden="true"
                  className="shrink-0 text-success"
                />
                <span className="text-xs text-text-default">{item}</span>
              </div>
            ))}
          </div>
          <div className="flex min-w-0 flex-col gap-compact">
            {[
              t('settings.account.notSent.saves'),
              t('settings.account.notSent.paths'),
            ].map((item) => (
              <div key={item} className="flex items-center gap-compact">
                <IconCircleX
                  size={14}
                  stroke={2}
                  aria-hidden="true"
                  className="shrink-0 text-text-muted"
                />
                <span className="text-xs text-text-dimmed">{item}</span>
              </div>
            ))}
          </div>
        </div>
      </SettingsCard>
    </SettingsPanel>
  );
};
