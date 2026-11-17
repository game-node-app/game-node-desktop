import React, { useMemo, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  IconAlertTriangle,
  IconCircleCheck,
  IconCircleX,
  IconCopy,
  IconCpu,
  IconDatabase,
  IconFolder,
  IconInfoCircle,
  IconPlayerPlay,
  IconPlugConnected,
  IconPuzzle,
  IconShieldCheck,
  IconSquare,
  IconUsersGroup,
  IconWorld,
  type TablerIcon,
} from '@tabler/icons-react';
import {
  commands,
  useSnapshot,
  type PluginInfo,
} from '../../../api';
import {
  Button,
  Pill,
  SegmentedControl,
  type PillTone,
} from '../../../components/ui';
import {
  pushToast,
  setSettingsTopic,
} from '../../../store/uiStore';
import { copyText } from '../../../diagnostics/crashCopy';
import { useI18n } from '../../../i18n';
import { providerLabel } from '../../../i18n/providerLabel';
import { IconBox, SettingsPanel } from '../components';
import { providerIcon } from '../StoreAccountCard';

export const PLUGIN_INVENTORY_QUERY_KEY = ['plugin-inventory'] as const;

type OriginFilter = 'all' | 'official' | 'community';

interface NativeAccessPill {
  icon?: TablerIcon;
  label: string;
}

function nativeAccessPill(
  access: string,
  t: (key: string) => string,
): NativeAccessPill {
  switch (access) {
    case 'network':
      return { icon: IconWorld, label: t('settings.plugins.native.network') };
    case 'files':
      return { icon: IconFolder, label: t('settings.plugins.native.files') };
    case 'process':
      return { icon: IconCpu, label: t('settings.plugins.native.processes') };
    case 'registry':
      return {
        icon: IconDatabase,
        label: t('settings.plugins.native.registry'),
      };
    case 'dbus':
      return {
        icon: IconPlugConnected,
        label: t('settings.plugins.native.dbus'),
      };
    default:
      return { label: access };
  }
}

function pluginPill(
  plugin: PluginInfo,
  t: (key: string) => string,
): { label: string; tone: PillTone; icon?: TablerIcon } {
  if (plugin.status !== 'installed') {
    switch (plugin.status) {
      case 'unavailable':
        return {
          label: t('settings.plugins.session.unavailable'),
          tone: 'neutral',
        };
      case 'failed':
        return {
          label: t('settings.plugins.session.failed'),
          tone: 'warning',
          icon: IconAlertTriangle,
        };
      case 'incompatible':
        return {
          label: t('settings.plugins.session.incompatible'),
          tone: 'warning',
          icon: IconAlertTriangle,
        };
      case 'missing':
        return {
          label: t('settings.plugins.session.missing'),
          tone: 'warning',
          icon: IconAlertTriangle,
        };
      default:
        return { label: plugin.status, tone: 'warning' };
    }
  }

  return plugin.session === 'active'
    ? {
        label: t('settings.plugins.session.active'),
        tone: 'success',
        icon: IconCircleCheck,
      }
    : {
        label: t('settings.plugins.session.stopped'),
        tone: 'neutral',
        icon: IconCircleX,
      };
}

/**
 * Plugins (S6): one card per trusted plugin descriptor with its version,
 * origin, copyable id, session state, Stop/Start, native-access pills and an
 * in-card details section for API grants versus native access. The origin
 * filter shows live counts, an expired store links to Lojas, and community
 * plugins carry the review warning (D14).
 */
export const PluginsPanel: React.FC = () => {
  const { t } = useI18n();
  const queryClient = useQueryClient();
  const { data: storeAccounts } = useSnapshot((snapshot) => snapshot.storeAccounts);
  const [origin, setOrigin] = useState<OriginFilter>('all');
  const [selectedPluginId, setSelectedPluginId] = useState<string | null>(null);
  const [pendingProvider, setPendingProvider] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);

  const { data = [], isPending, isError } = useQuery({
    queryKey: PLUGIN_INVENTORY_QUERY_KEY,
    queryFn: commands.pluginInventory,
    staleTime: 30_000,
  });

  const counts = useMemo(
    () => ({
      all: data.length,
      official: data.filter((plugin) => plugin.source === 'official').length,
      community: data.filter((plugin) => plugin.source === 'community').length,
    }),
    [data],
  );

  const visiblePlugins =
    origin === 'all'
      ? data
      : data.filter((plugin) => plugin.source === origin);

  const expiredProviderIds = new Set(
    (storeAccounts ?? [])
      .filter((account) => account.state === 'expired')
      .map((account) => account.provider),
  );

  const copyIdentifier = async (pluginId: string) => {
    const copied = await copyText(pluginId);
    pushToast(
      copied ? 'success' : 'danger',
      copied
        ? t('settings.plugins.identifierCopied')
        : t('Could not copy to the clipboard.'),
    );
  };

  const toggleSession = async (plugin: PluginInfo) => {
    if (pendingProvider) return;
    setPendingProvider(plugin.providerId);
    setActionError(null);
    try {
      if (plugin.session === 'active') {
        await commands.pluginStop(plugin.providerId);
      } else {
        await commands.pluginStart(plugin.providerId);
      }
      await queryClient.invalidateQueries({
        queryKey: PLUGIN_INVENTORY_QUERY_KEY,
      });
    } catch {
      setActionError(t('settings.plugins.actionFailed'));
    } finally {
      setPendingProvider(null);
    }
  };

  return (
    <SettingsPanel
      title={t('settings.nav.plugins')}
      description={t('settings.plugins.description')}
      action={
        !isPending && !isError && data.length > 0 ? (
          <SegmentedControl<OriginFilter>
            aria-label={t('settings.plugins.originFilter')}
            segmentPadding="filter"
            value={origin}
            onChange={setOrigin}
            data={[
              {
                value: 'all',
                label: t('settings.plugins.filter.all', { n: counts.all }),
              },
              {
                value: 'official',
                label: t('settings.plugins.filter.official', {
                  n: counts.official,
                }),
              },
              {
                value: 'community',
                label: t('settings.plugins.filter.community', {
                  n: counts.community,
                }),
              },
            ]}
          />
        ) : undefined
      }
    >
      {actionError ? (
        <div
          role="alert"
          data-testid="settings-plugins-error"
          className="flex items-start gap-compact rounded-md border border-danger bg-danger-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-danger"
          />
          <span>{actionError}</span>
        </div>
      ) : null}

      {isPending ? (
        <p role="status" className="text-body text-text-secondary">
          {t('plugins.loading')}
        </p>
      ) : isError ? (
        <p
          role="alert"
          className="flex items-start gap-compact rounded-md border border-danger bg-danger-subtle p-sm text-xs leading-note text-text-secondary"
        >
          <IconAlertTriangle
            size={16}
            stroke={2}
            aria-hidden="true"
            className="shrink-0 text-danger"
          />
          <span>{t('plugins.loadFailed')}</span>
        </p>
      ) : data.length === 0 ? (
        <p className="text-body text-text-secondary">{t('plugins.empty')}</p>
      ) : (
        visiblePlugins.map((plugin) => {
          const name = providerLabel(plugin.providerId, plugin.name, t);
          const isCommunity = plugin.source === 'community';
          const ProviderIcon = isCommunity
            ? IconPuzzle
            : providerIcon(plugin.providerId);
          const state = pluginPill(plugin, t);
          const expanded = selectedPluginId === plugin.pluginId;
          const expired = expiredProviderIds.has(plugin.providerId);
          const headingId = `plugin-heading-${plugin.pluginId}`;
          const detailsId = `plugin-details-${plugin.pluginId}`;
          const canToggleSession = plugin.status === 'installed';
          const isActive = plugin.session === 'active';

          return (
            <article
              key={plugin.pluginId}
              data-plugin-id={plugin.pluginId}
              data-selected={expanded ? 'true' : undefined}
              aria-labelledby={headingId}
              className={`flex flex-col overflow-hidden rounded-card border bg-secondary ${
                expired
                  ? 'border-warning'
                  : expanded
                    ? 'border-brand'
                    : 'border-border-subtle'
              }`}
            >
              <div className="flex flex-wrap items-center gap-row p-md">
                <IconBox icon={ProviderIcon} />
                <div className="flex min-w-0 flex-1 flex-col gap-[4px]">
                  <div className="flex flex-wrap items-center gap-compact">
                    <h3
                      id={headingId}
                      className="truncate text-title font-semibold text-text-primary"
                    >
                      {name}
                    </h3>
                    <span className="text-xs text-text-dimmed">
                      {plugin.version}
                    </span>
                    <Pill
                      tone={isCommunity ? 'neutral' : 'brand'}
                      icon={isCommunity ? IconUsersGroup : IconShieldCheck}
                    >
                      {isCommunity
                        ? t('settings.plugins.origin.community')
                        : t('settings.plugins.origin.official')}
                    </Pill>
                  </div>
                  <button
                    type="button"
                    aria-label={`${t('settings.plugins.copyIdentifier')}: ${plugin.pluginId}`}
                    title={plugin.pluginId}
                    className="flex w-fit max-w-full items-center gap-[6px] rounded-sm text-xs text-text-dimmed transition-colors hover:text-text-default focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                    onClick={() => void copyIdentifier(plugin.pluginId)}
                  >
                    <IconCopy
                      size={12}
                      stroke={2}
                      aria-hidden="true"
                      className="shrink-0"
                    />
                    <span className="truncate">{plugin.pluginId}</span>
                  </button>
                </div>

                <Pill tone={state.tone} icon={state.icon}>
                  {state.label}
                </Pill>
                {canToggleSession ? (
                  <Button
                    size="sm"
                    variant="secondary"
                    loading={pendingProvider === plugin.providerId}
                    disabled={pendingProvider !== null}
                    leftIcon={isActive ? IconSquare : IconPlayerPlay}
                    onClick={() => void toggleSession(plugin)}
                  >
                    {isActive
                      ? t('settings.plugins.stop')
                      : t('settings.plugins.start')}
                  </Button>
                ) : null}
              </div>

              {expired ? (
                <button
                  type="button"
                  aria-label={t('settings.plugins.expiredSession')}
                  className="flex w-full items-center gap-compact border-t border-border-subtle bg-warning-subtle px-md py-[10px] text-left text-xs leading-note text-text-default transition-colors hover:bg-warning/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-brand"
                  onClick={() => setSettingsTopic('stores')}
                >
                  <IconAlertTriangle
                    size={14}
                    stroke={2}
                    aria-hidden="true"
                    className="shrink-0 text-warning"
                  />
                  <span>{t('settings.plugins.expiredSession')}</span>
                </button>
              ) : null}

              <div className="flex flex-wrap items-center gap-compact border-t border-border-subtle px-md py-sm">
                <span className="text-xs text-text-secondary">
                  {t('settings.plugins.permissions')}
                </span>
                {plugin.nativeAccess.map((access) => {
                  const pill = nativeAccessPill(access, t);
                  return (
                    <Pill key={access} tone="neutral" icon={pill.icon}>
                      {pill.label}
                    </Pill>
                  );
                })}
                <button
                  type="button"
                  aria-expanded={expanded}
                  aria-controls={detailsId}
                  className="ml-auto rounded-sm text-xs font-medium text-brand transition-colors hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand"
                  onClick={() =>
                    setSelectedPluginId(expanded ? null : plugin.pluginId)
                  }
                >
                  {expanded
                    ? t('settings.plugins.hideDetails')
                    : t('settings.plugins.details')}
                </button>
              </div>

              <div
                id={detailsId}
                hidden={!expanded}
                className={
                  expanded
                    ? 'flex flex-col gap-sm border-t border-border-subtle px-md py-sm'
                    : 'hidden'
                }
              >
                {expanded ? (
                  <>
                    <div className="flex flex-col gap-compact">
                      <h4 className="text-xs font-medium text-text-secondary">
                        {t('settings.plugins.apiPermissions')}
                      </h4>
                      <ul
                        aria-label={t('settings.plugins.apiPermissions')}
                        className="flex flex-wrap gap-compact"
                      >
                        {plugin.capabilities.map((grant) => (
                          <li key={grant}>
                            <Pill tone="neutral">{grant}</Pill>
                          </li>
                        ))}
                      </ul>
                    </div>
                    <div className="flex flex-col gap-compact">
                      <h4 className="text-xs font-medium text-text-secondary">
                        {t('settings.plugins.nativeAccess')}
                      </h4>
                      <ul
                        aria-label={t('settings.plugins.nativeAccess')}
                        className="flex flex-wrap gap-compact"
                      >
                        {plugin.nativeAccess.map((access) => {
                          const pill = nativeAccessPill(access, t);
                          return (
                            <li key={access}>
                              <Pill tone="neutral" icon={pill.icon}>
                                {pill.label}
                              </Pill>
                            </li>
                          );
                        })}
                      </ul>
                    </div>
                  </>
                ) : null}
              </div>
            </article>
          );
        })
      )}

      <div className="flex items-start gap-compact rounded-md border border-border-subtle bg-surface p-sm text-xs leading-note text-text-secondary">
        <IconInfoCircle
          size={16}
          stroke={2}
          aria-hidden="true"
          className="mt-[1px] shrink-0"
        />
        <span>{t('settings.plugins.communityNote')}</span>
      </div>
    </SettingsPanel>
  );
};
