import {
  IconArchive,
  IconDeviceGamepad2,
  IconFolder,
  IconInfoCircle,
  IconPuzzle,
  IconSettings,
  IconTool,
  IconUser,
  type TablerIcon,
} from '@tabler/icons-react';
import type { AppInfo } from '../../api/types';
import type { SettingsTopic } from '../../store/uiStore';
import { normalizeSettingsSearch, settingsTextMatches } from './search';

export type SettingsTopicGroup = 'account' | 'library' | 'app';

export interface SettingsTopicDefinition {
  id: SettingsTopic;
  group: SettingsTopicGroup;
  icon: TablerIcon;
  /** i18n key of the navigation label and the panel title. */
  titleKey: string;
  /** i18n key of the panel description. */
  descriptionKey: string;
  /** i18n keys of the row labels a search can match in this topic. */
  searchableRowKeys: string[];
  /**
   * False when this platform cannot use the topic. The topic stays in the
   * navigation and its panel explains that it is unavailable (`desktop-settings`
   * › Navegação por tópicos). Host capability flags arrive with the panels.
   */
  available: (appInfo: AppInfo | null | undefined) => boolean;
}

export interface SettingsNavGroup {
  id: SettingsTopicGroup;
  labelKey: string;
  topics: SettingsTopicDefinition[];
}

/**
 * The eight topics in the order the specification lists them:
 * Conta (Conta e sincronização) · Biblioteca (Lojas, Jogos DRM-free, Backups) ·
 * Aplicativo (Geral, Ferramentas, Plugins, Sobre).
 */
export const SETTINGS_NAV_GROUPS: SettingsNavGroup[] = [
  {
    id: 'account',
    labelKey: 'settings.shell.navGroup.account',
    topics: [
      {
        id: 'account',
        group: 'account',
        icon: IconUser,
        titleKey: 'settings.nav.accountSync',
        descriptionKey: 'settings.account.description',
        searchableRowKeys: [
          'settings.account.syncWithGamenode',
          'settings.account.playtimeSource',
          'settings.account.lastSync',
          'settings.account.whatIsSynced',
          'settings.account.openProfile',
        ],
        available: () => true,
      },
    ],
  },
  {
    id: 'library',
    labelKey: 'settings.shell.navGroup.library',
    topics: [
      {
        id: 'stores',
        group: 'library',
        icon: IconDeviceGamepad2,
        titleKey: 'settings.nav.stores',
        descriptionKey: 'settings.stores.description',
        searchableRowKeys: [
          'settings.stores.includeShared',
          'settings.stores.credentials',
          'settings.stores.detected',
          'settings.stores.rescan',
          'settings.stores.showHidden',
        ],
        available: () => true,
      },
      {
        id: 'drmFree',
        group: 'library',
        icon: IconFolder,
        titleKey: 'settings.nav.drmFree',
        descriptionKey: 'settings.drmFree.description',
        searchableRowKeys: [
          'settings.drmFree.checkAll',
          'settings.drmFree.watchedFolders',
          'settings.drmFree.addFolder',
          'settings.drmFree.discovery.onStart',
          'settings.drmFree.discovery.realtime',
          'settings.drmFree.ignored',
          'settings.drmFree.addFromExecutable',
        ],
        available: () => true,
      },
      {
        id: 'backups',
        group: 'library',
        icon: IconArchive,
        titleKey: 'settings.nav.backups',
        descriptionKey: 'settings.backups.description',
        searchableRowKeys: [
          'settings.backups.protectAutomatically',
          'settings.backups.destination',
          'settings.backups.copiesKept',
          'settings.backups.spaceUsed',
        ],
        available: () => true,
      },
    ],
  },
  {
    id: 'app',
    labelKey: 'settings.shell.navGroup.app',
    topics: [
      {
        id: 'general',
        group: 'app',
        icon: IconSettings,
        titleKey: 'settings.nav.general',
        descriptionKey: 'settings.general.description',
        searchableRowKeys: [
          'settings.general.startWithWindows',
          'settings.general.startMinimized',
          'settings.general.closeToTray',
          'settings.general.defaultView',
          'settings.general.coverSize',
          'settings.general.openIn',
          'settings.general.showStatusBar',
          'settings.general.language',
          'settings.general.shortcuts',
        ],
        available: () => true,
      },
      {
        id: 'tools',
        group: 'app',
        icon: IconTool,
        titleKey: 'settings.nav.tools',
        descriptionKey: 'settings.tools.description',
        searchableRowKeys: [
          'settings.tools.supportedTools',
          'settings.tools.autoCheck',
          'settings.tools.defaultChannel',
          'settings.tools.downloadCache',
          'settings.tools.clearCache',
        ],
        available: () => true,
      },
      {
        id: 'plugins',
        group: 'app',
        icon: IconPuzzle,
        titleKey: 'settings.nav.plugins',
        descriptionKey: 'settings.plugins.description',
        searchableRowKeys: [
          'settings.plugins.filter.all',
          'settings.plugins.permissions',
          'settings.plugins.details',
          'settings.plugins.communityNote',
        ],
        available: () => true,
      },
      {
        id: 'about',
        group: 'app',
        icon: IconInfoCircle,
        titleKey: 'settings.nav.about',
        descriptionKey: 'settings.about.description',
        searchableRowKeys: [
          'settings.about.diagnostics',
          'settings.about.recentLog',
          'settings.about.systemInfo',
          'settings.about.licenses',
          'settings.about.showLicenseNotices',
          'settings.about.links.website',
        ],
        available: () => true,
      },
    ],
  },
];

/** Flattened registry in specification order (eight topics). */
export const SETTINGS_TOPICS: SettingsTopicDefinition[] =
  SETTINGS_NAV_GROUPS.flatMap((group) => group.topics);

export function settingsTopicDefinition(
  topic: SettingsTopic,
): SettingsTopicDefinition {
  return (
    SETTINGS_TOPICS.find((definition) => definition.id === topic) ??
    SETTINGS_TOPICS[0]
  );
}

export function isSettingsTopicAvailable(
  definition: SettingsTopicDefinition,
  appInfo: AppInfo | null | undefined,
): boolean {
  return definition.available(appInfo);
}

/** The translated text a topic search compares against. */
export function settingsTopicSearchText(
  definition: SettingsTopicDefinition,
  t: (key: string) => string,
): string {
  return [
    t(definition.titleKey),
    t(definition.descriptionKey),
    ...definition.searchableRowKeys.map((key) => t(key)),
  ].join('\n');
}

export function settingsTopicMatches(
  definition: SettingsTopicDefinition,
  normalizedQuery: string,
  t: (key: string) => string,
): boolean {
  return settingsTextMatches(
    settingsTopicSearchText(definition, t),
    normalizedQuery,
  );
}

/** Topics to show in the navigation for a query; the empty query keeps all. */
export function settingsTopicsForSearch(
  query: string,
  t: (key: string) => string,
): SettingsTopicDefinition[] {
  const normalizedQuery = normalizeSettingsSearch(query);
  if (normalizedQuery.length === 0) return SETTINGS_TOPICS;
  return SETTINGS_TOPICS.filter((definition) =>
    settingsTopicMatches(definition, normalizedQuery, t),
  );
}

export interface SettingsVisibleNavGroup {
  id: SettingsTopicGroup;
  labelKey: string;
  topics: SettingsTopicDefinition[];
}

/** Navigation groups with the topics that match `query`, empty groups dropped. */
export function settingsNavGroupsForSearch(
  query: string,
  t: (key: string) => string,
): SettingsVisibleNavGroup[] {
  const visible = new Set(
    settingsTopicsForSearch(query, t).map((topic) => topic.id),
  );
  return SETTINGS_NAV_GROUPS.map((group) => ({
    id: group.id,
    labelKey: group.labelKey,
    topics: group.topics.filter((topic) => visible.has(topic.id)),
  })).filter((group) => group.topics.length > 0);
}
