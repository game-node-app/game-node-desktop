import React, { useEffect, useRef, useState } from 'react';
import {
  IconAlertTriangle,
  IconCheck,
  IconLock,
} from '@tabler/icons-react';
import {
  commands,
  useLibraryView,
  useSnapshot,
} from '../../../api';
import { Button, Pill } from '../../../components/ui';
import { providerLabel } from '../../../i18n/providerLabel';
import { pushToast } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import { StoreAccountCard, providerIcon } from '../StoreAccountCard';
import {
  SettingsCard,
  SettingsPanel,
  SettingsRow,
} from '../components';

interface DetectedStore {
  id: string;
  name: string;
  count: number;
  installedCount: number;
  ownedCount: number;
  installRoot?: string | null;
  libraryFolders?: number | null;
  /** The last local scan found the store; catalog facts never count as detection. */
  detected: boolean;
  failed: boolean;
}

/**
 * Lojas (S2, D10): one provider-neutral account card per connected store, the
 * credential-storage card and "Detectadas neste PC" with each scanned store's
 * install location, library-folder count and scan result. DRM-free discovery
 * moved to its own topic and playtime source to Conta e sincronização.
 */
export const StoresPanel: React.FC = () => {
  const { t } = useI18n();
  const library = useLibraryView();
  const { data: storeAccounts } = useSnapshot((state) => state.storeAccounts);
  const { data: sessionStorage } = useSnapshot((state) => state.storage);
  const { data: sharedExcluded } = useSnapshot(
    (state) => state.preferences.sharedExcluded,
  );

  const signInPending = useRef(new Set<string>());
  const lastSyncedAt = useRef(new Map<string, number | null>());
  const autoCheckedFor = useRef(new Map<string, number>());

  const [sharedOverrides, setSharedOverrides] = useState<
    Record<string, boolean>
  >({});
  const [sharedError, setSharedError] = useState<string | null>(null);

  const providers = library?.providers ?? [];
  const failedProviders = library?.failedProviders ?? [];
  const failedFromScan = new Set(
    (library?.scanProgress ?? [])
      .filter((entry) => entry.state === 'failed')
      .map((entry) => entry.provider),
  );
  const hasFailed = (id: string) =>
    failedProviders.includes(id) || failedFromScan.has(id);

  const accounts = storeAccounts ?? [];
  const providerName = (id: string): string =>
    providerLabel(
      id,
      providers.find((provider) => provider.id === id)?.name,
      t,
    );

  // A connected account older than the freshness interval is validated (and its
  // catalog refreshed) once, without a second request while it runs.
  useEffect(() => {
    const now = Math.floor(Date.now() / 1000);
    for (const account of storeAccounts ?? []) {
      if (account.state !== 'connected') {
        continue;
      }
      const lastChecked = account.lastCheckedAt ?? 0;
      if (
        now - lastChecked > 5 * 60 &&
        autoCheckedFor.current.get(account.provider) !== lastChecked
      ) {
        autoCheckedFor.current.set(account.provider, lastChecked);
        void commands.providerCheck(account.provider);
      }
    }
  }, [storeAccounts]);

  useEffect(() => {
    for (const account of storeAccounts ?? []) {
      if (account.state === 'signingIn') {
        signInPending.current.add(account.provider);
        lastSyncedAt.current.delete(account.provider);
        continue;
      }
      if (account.state === 'connected') {
        // A sign-in validates the session before the catalog is fetched, so the
        // first connected snapshot still has the previous (usually zero)
        // counts. Announce the import only once a catalog sync has completed.
        const syncedAt = account.lastSyncedAt ?? null;
        if (
          signInPending.current.has(account.provider) &&
          syncedAt !== null &&
          syncedAt !== lastSyncedAt.current.get(account.provider)
        ) {
          signInPending.current.delete(account.provider);
          const imported =
            account.ownedCount +
            (account.sharedLabel !== null ? account.sharedCount : 0);
          pushToast(
            'success',
            t('settings.storeConnectedToast', {
              provider: providerName(account.provider),
              count: imported,
            }),
          );
        }
        lastSyncedAt.current.set(account.provider, syncedAt);
        continue;
      }
      if (account.state !== 'checking') {
        signInPending.current.delete(account.provider);
        lastSyncedAt.current.delete(account.provider);
      }
    }
  });

  // Drop an optimistic shared-games value once the snapshot confirms it.
  useEffect(() => {
    setSharedOverrides((previous) => {
      const entries = Object.entries(previous);
      if (entries.length === 0) return previous;
      const next = { ...previous };
      let changed = false;
      for (const [provider, value] of entries) {
        if ((!sharedExcluded.includes(provider)) === value) {
          delete next[provider];
          changed = true;
        }
      }
      return changed ? next : previous;
    });
  }, [sharedExcluded]);

  const includeSharedFor = (provider: string): boolean =>
    sharedOverrides[provider] ?? !sharedExcluded.includes(provider);

  const handleIncludeShared = async (provider: string, checked: boolean) => {
    const nextExcluded = new Set(sharedExcluded);
    for (const [id, included] of Object.entries(sharedOverrides)) {
      if (included) nextExcluded.delete(id);
      else nextExcluded.add(id);
    }
    if (checked) nextExcluded.delete(provider);
    else nextExcluded.add(provider);

    setSharedOverrides((previous) => ({ ...previous, [provider]: checked }));
    setSharedError(null);
    try {
      await commands.setAppPreference({
        sharedExcluded: [...nextExcluded],
      });
    } catch {
      setSharedOverrides((previous) => {
        const copy = { ...previous };
        delete copy[provider];
        return copy;
      });
      setSharedError(t('settings.stores.saveSharedFailed'));
    }
  };

  const handleRescan = () => {
    void commands.rescanLibrary();
  };

  const handleShowHidden = () => {
    void commands.libraryInput('resetExcluded');
    pushToast('success', t('settings.stores.showHidden'));
  };

  const handleSignOut = (provider: string) => {
    pushToast(
      'success',
      t('settings.storeSignedOutToast', { provider: providerName(provider) }),
    );
  };

  const detected: DetectedStore[] = providers.map((provider) => ({
    ...provider,
    detected: provider.detected ?? false,
    failed: hasFailed(provider.id),
  }));
  const known = new Set(detected.map((store) => store.id));
  for (const id of [...failedProviders, ...failedFromScan]) {
    if (known.has(id)) continue;
    known.add(id);
    detected.push({
      id,
      name: providerName(id),
      count: 0,
      installedCount: 0,
      ownedCount: 0,
      detected: false,
      failed: true,
    });
  }

  const credential = (() => {
    switch (sessionStorage) {
      case 'windowsCredentialManager':
        return {
          label: t('settings.stores.credentials.windows'),
          help: t('settings.stores.credentials.help'),
          available: true,
        };
      case 'secretService':
        return {
          label: t('settings.stores.credentials.keyring'),
          help: t('settings.stores.credentials.keyringHelp'),
          available: true,
        };
      case 'privateFile':
      default:
        return {
          label: t('settings.stores.credentials.privateFile'),
          help: t('settings.stores.credentials.privateFileHelp'),
          available: false,
        };
    }
  })();

  return (
    <SettingsPanel
      title={t('settings.nav.stores')}
      description={t('settings.stores.description')}
    >
      {sharedError ? (
        <div
          role="alert"
          data-testid="settings-stores-error"
          className="flex items-start gap-compact rounded-md border border-danger bg-danger-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-danger"
          />
          <span>{sharedError}</span>
        </div>
      ) : null}

      {accounts.map((account) => (
        <StoreAccountCard
          key={account.provider}
          account={{ ...account, storage: account.storage ?? sessionStorage }}
          providerName={providerName(account.provider)}
          includeShared={includeSharedFor(account.provider)}
          onIncludeSharedChange={(checked) =>
            void handleIncludeShared(account.provider, checked)
          }
          onSignOut={() => handleSignOut(account.provider)}
        />
      ))}

      <SettingsCard title={t('settings.stores.credentials')}>
        <SettingsRow
          compact
          icon={IconLock}
          label={credential.label}
          help={credential.help}
          control={
            <Pill
              tone={credential.available ? 'success' : 'warning'}
              icon={credential.available ? IconLock : IconAlertTriangle}
            >
              {credential.available
                ? t('settings.stores.credentials.available')
                : t('settings.stores.credentials.unavailable')}
            </Pill>
          }
        />
      </SettingsCard>

      <SettingsCard
        title={t('settings.stores.detected')}
        aria-busy={library?.scanning ?? false}
        action={
          <Button
            size="sm"
            variant="secondary"
            loading={library?.scanning ?? false}
            onClick={handleRescan}
          >
            {t('settings.stores.rescan')}
          </Button>
        }
      >
        {detected.length === 0 ? (
          <p className="border-t border-border-subtle px-md py-sm text-xs text-text-dimmed">
            {t('settings.noInstalledGamesFound')}
          </p>
        ) : (
          detected.map((store) => {
            const StoreIcon = providerIcon(store.id);
            const meta =
              store.installRoot != null
                ? store.libraryFolders != null
                  ? t('settings.stores.detected.meta', {
                      path: store.installRoot,
                      n: store.libraryFolders,
                    })
                  : store.installRoot
                : store.installedCount > 0
                  ? t('settings.stores.detected.installedOnly', {
                      n: store.installedCount,
                    })
                  : null;
            const state = store.failed
              ? {
                  tone: 'warning' as const,
                  label: t('settings.stores.detected.state.scanFailed'),
                  icon: IconAlertTriangle,
                }
              : store.detected
                ? {
                    tone: 'neutral' as const,
                    label: t('settings.stores.detected.state.detected'),
                    icon: IconCheck,
                  }
                : {
                    tone: 'neutral' as const,
                    label: t('settings.stores.detected.state.notFound'),
                    icon: undefined,
                  };
            return (
              <SettingsRow
                key={store.id}
                compact
                icon={StoreIcon}
                label={store.name}
                help={meta ?? undefined}
                control={
                  <Pill tone={state.tone} icon={state.icon}>
                    {state.label}
                  </Pill>
                }
              />
            );
          })
        )}
        <SettingsRow
          compact
          label={t('settings.stores.showHidden')}
          control={
            <Button
              size="sm"
              variant="secondary"
              onClick={handleShowHidden}
            >
              {t('settings.stores.showHidden')}
            </Button>
          }
        />
      </SettingsCard>
    </SettingsPanel>
  );
};
