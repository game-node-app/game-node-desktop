import {
  useBootstrapState,
  useLibraryView,
  useLocalGames,
} from '../../api/useSnapshot';
import { commands } from '../../api/commands';
import type { LibraryView, LocalView } from '../../api/types';
import { openSettingsModal, uiStore, useUiStore } from '../../store/uiStore';
import { useI18n } from '../../i18n';
import { ResizeHandle } from './ResizeHandle';

export interface SidebarProps {
  library?: LibraryView | null;
  localGames?: LocalView | null;
  version?: string;
  className?: string;
}

export function Sidebar({
  library: propLibrary,
  localGames: propLocalGames,
  version: propVersion,
  className,
}: SidebarProps = {}) {
  const { t } = useI18n();
  const hookLibrary = useLibraryView();
  const hookLocalGames = useLocalGames();
  const bootstrap = useBootstrapState();
  const dragWidth = useUiStore((s) => s.sidebarDragWidth);

  const library = propLibrary !== undefined ? propLibrary : hookLibrary;
  const localGames = propLocalGames !== undefined ? propLocalGames : hookLocalGames;
  const version =
    propVersion ?? bootstrap?.data?.version ?? '0.1.0';

  const storedWidth = library?.layout?.sidebarWidth ?? 216;
  const rawWidth = dragWidth ?? storedWidth;
  const clampedWidth = Math.min(360, Math.max(160, Math.round(rawWidth)));

  const activeFilter = library?.layout?.filter ?? 'all';
  const isAllActive = activeFilter === 'all';
  const isProviderActive = (id: string) =>
    typeof activeFilter === 'object' &&
    activeFilter !== null &&
    'provider' in activeFilter &&
    activeFilter.provider === id;

  const failedProviders = library?.failedProviders ?? [];
  const isProviderFailed = (id: string, name: string) =>
    failedProviders.includes(id) || failedProviders.includes(name);

  const candidates = (localGames?.candidates ?? []).filter((c) => !c.ignored);
  const hasCandidates = candidates.length > 0;

  const handleSelectAll = () => {
    commands.libraryInput({ setFilter: 'all' });
  };

  const handleSelectProvider = (providerId: string) => {
    commands.libraryInput({ setFilter: { provider: providerId } });
  };

  const handleAddGame = () => {
    commands.pickLocalExecutable(null);
  };

  const handleOpenDiscovery = () => {
    uiStore.setState((s) => ({ ...s, discoveryModalOpen: true }));
  };

  const handleOpenSettings = () => {
    openSettingsModal('account');
  };

  return (
    <aside
      data-testid="sidebar"
      className={className}
      style={{
        width: `${clampedWidth}px`,
        minWidth: '160px',
        maxWidth: '360px',
        height: '100%',
        backgroundColor: 'var(--color-chrome, #111111)',
        borderRight: '1px solid var(--color-border-subtle, #2A2A2A)',
        display: 'flex',
        flexDirection: 'row',
        position: 'relative',
        boxSizing: 'border-box',
        userSelect: 'none',
        flexShrink: 0,
      }}
    >
      {/* Sidebar Content */}
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          flex: 1,
          height: '100%',
          overflow: 'hidden',
          padding: '16px',
          boxSizing: 'border-box',
          gap: '16px',
        }}
      >
        {/* Navigation Section */}
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: '4px',
            flex: 1,
            overflowY: 'auto',
          }}
        >
          <div
            style={{
              fontSize: '11px',
              fontWeight: 600,
              letterSpacing: '0.8px',
              color: 'var(--color-text-dimmed, #808080)',
              marginBottom: '6px',
              textTransform: 'uppercase',
            }}
          >
            {t('YOUR SPACE')}
          </div>

          {/* All Games Item */}
          <button
            type="button"
            data-testid="sidebar-all-games"
            onClick={handleSelectAll}
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              width: '100%',
              padding: '8px 12px',
              backgroundColor: isAllActive
                ? 'var(--color-accent-subtle, rgba(241, 80, 37, 0.12))'
                : 'transparent',
              color: isAllActive
                ? 'var(--color-primary, #F15025)'
                : 'var(--color-text-default, #D9D9D9)',
              border: isAllActive
                ? '1px solid var(--color-primary, #F15025)'
                : '1px solid transparent',
              borderRadius: 'var(--radius-md, 8px)',
              fontSize: 'var(--font-size-sm, 14px)',
              fontWeight: isAllActive ? 600 : 400,
              cursor: 'pointer',
              textAlign: 'left',
              transition: 'background-color 0.15s',
            }}
          >
            <span>{t('All games')}</span>
            <span
              data-testid="sidebar-all-count"
              style={{
                fontSize: '12px',
                color: isAllActive
                  ? 'var(--color-primary, #F15025)'
                  : 'var(--color-text-dimmed, #808080)',
              }}
            >
              {library?.total ?? 0}
            </span>
          </button>

          {/* Provider Items */}
          {library?.providers && library.providers.length > 0 && (
            <div
              data-testid="sidebar-providers-list"
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: '2px',
                marginTop: '4px',
              }}
            >
              {library.providers.map((provider) => {
                const active = isProviderActive(provider.id);
                const failed = isProviderFailed(provider.id, provider.name);

                return (
                  <button
                    key={provider.id}
                    type="button"
                    data-testid={`sidebar-provider-${provider.id}`}
                    onClick={() => handleSelectProvider(provider.id)}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      width: '100%',
                      padding: '7px 12px',
                      backgroundColor: active
                        ? 'var(--color-accent-subtle, rgba(241, 80, 37, 0.12))'
                        : 'transparent',
                      color: active
                        ? 'var(--color-primary, #F15025)'
                        : 'var(--color-text-default, #D9D9D9)',
                      border: active
                        ? '1px solid var(--color-primary, #F15025)'
                        : '1px solid transparent',
                      borderRadius: 'var(--radius-md, 8px)',
                      fontSize: 'var(--font-size-sm, 14px)',
                      fontWeight: active ? 600 : 400,
                      cursor: 'pointer',
                      textAlign: 'left',
                    }}
                  >
                    <div
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: '6px',
                        overflow: 'hidden',
                      }}
                    >
                      <span
                        style={{
                          whiteSpace: 'nowrap',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                        }}
                      >
                        {provider.name}
                      </span>
                      {failed && (
                        <span
                          data-testid={`provider-warning-${provider.id}`}
                          aria-label="Provider sync failure"
                          title="Provider sync failure"
                          style={{
                            color: 'var(--color-warning, #F5A524)',
                            fontSize: '12px',
                          }}
                        >
                          ⚠️
                        </span>
                      )}
                    </div>
                    <span
                      style={{
                        fontSize: '12px',
                        color: active
                          ? 'var(--color-primary, #F15025)'
                          : 'var(--color-text-dimmed, #808080)',
                        marginLeft: '8px',
                      }}
                    >
                      {provider.count}
                    </span>
                  </button>
                );
              })}
            </div>
          )}

          {/* DRM-Free & Local Actions */}
          <div
            style={{
              display: 'flex',
              flexDirection: 'column',
              gap: '8px',
              marginTop: '16px',
            }}
          >
            <button
              type="button"
              data-testid="sidebar-add-game"
              onClick={handleAddGame}
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                gap: '8px',
                width: '100%',
                padding: '8px 12px',
                backgroundColor: 'transparent',
                color: 'var(--color-text-default, #D9D9D9)',
                border: '1px dashed var(--color-border-subtle, #2A2A2A)',
                borderRadius: 'var(--radius-md, 8px)',
                fontSize: '13px',
                cursor: 'pointer',
              }}
            >
              <span>+ {t('Add a game')}</span>
            </button>

            {hasCandidates && (
              <button
                type="button"
                data-testid="sidebar-discovery-button"
                onClick={handleOpenDiscovery}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  width: '100%',
                  padding: '8px 12px',
                  backgroundColor: 'var(--color-surface, #262525)',
                  color: 'var(--color-text-default, #D9D9D9)',
                  border: '1px solid var(--color-border-subtle, #2A2A2A)',
                  borderRadius: 'var(--radius-md, 8px)',
                  fontSize: '13px',
                  cursor: 'pointer',
                }}
              >
                <span>{t('Discovered games')}</span>
                <span
                  style={{
                    backgroundColor: 'var(--color-primary, #F15025)',
                    color: 'var(--color-on-primary, #EDEDED)',
                    padding: '1px 6px',
                    borderRadius: 'var(--radius-full, 9999px)',
                    fontSize: '11px',
                    fontWeight: 600,
                  }}
                >
                  {candidates.length}
                </span>
              </button>
            )}
          </div>
        </div>

        {/* Footer: Settings & Version */}
        <div
          style={{
            borderTop: '1px solid var(--color-border-subtle, #2A2A2A)',
            paddingTop: '12px',
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
          }}
        >
          <button
            type="button"
            data-testid="sidebar-settings-button"
            onClick={handleOpenSettings}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: '8px',
              width: '100%',
              padding: '8px 12px',
              backgroundColor: 'transparent',
              color: 'var(--color-text-default, #D9D9D9)',
              border: 'none',
              borderRadius: 'var(--radius-md, 8px)',
              fontSize: 'var(--font-size-sm, 14px)',
              cursor: 'pointer',
              textAlign: 'left',
            }}
          >
            <span>⚙ {t('Settings')}</span>
          </button>

          <div
            data-testid="sidebar-version-label"
            style={{
              fontSize: '11px',
              color: 'var(--color-text-dimmed, #808080)',
              paddingLeft: '12px',
            }}
          >
            GameNode · {version}
          </div>
        </div>
      </div>

      {/* Resizable handle on the right edge */}
      <ResizeHandle currentWidth={clampedWidth} />
    </aside>
  );
}
