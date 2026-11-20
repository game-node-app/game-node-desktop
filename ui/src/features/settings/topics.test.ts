import { describe, expect, it } from 'vitest';
import { ptBR } from '../../i18n/pt-BR';
import { normalizeSettingsSearch } from './search';
import {
  SETTINGS_NAV_GROUPS,
  SETTINGS_TOPICS,
  isSettingsTopicAvailable,
  settingsNavGroupsForSearch,
  settingsTopicsForSearch,
} from './topics';

const t = (key: string): string =>
  (ptBR as Record<string, string>)[key] ?? key;

describe('settings topic registry', () => {
  it('lists the eight topics in specification order', () => {
    expect(SETTINGS_TOPICS.map((topic) => topic.id)).toEqual([
      'account',
      'stores',
      'drmFree',
      'backups',
      'general',
      'tools',
      'plugins',
      'about',
    ]);
  });

  it('groups the topics as Conta / Biblioteca / Aplicativo', () => {
    expect(
      SETTINGS_NAV_GROUPS.map((group) => [
        group.labelKey,
        group.topics.map((topic) => topic.id),
      ]),
    ).toEqual([
      ['settings.shell.navGroup.account', ['account']],
      ['settings.shell.navGroup.library', ['stores', 'drmFree', 'backups']],
      ['settings.shell.navGroup.app', ['general', 'tools', 'plugins', 'about']],
    ]);
  });

  it('hides no topic for any platform or missing AppInfo', () => {
    const appInfo = { locale: 'en', version: '0.2.0', backgroundSupported: false };
    for (const definition of SETTINGS_TOPICS) {
      expect(isSettingsTopicAvailable(definition, appInfo)).toBe(true);
      expect(
        isSettingsTopicAvailable(definition, { ...appInfo, platform: 'linux' }),
      ).toBe(true);
      expect(isSettingsTopicAvailable(definition, null)).toBe(true);
    }
    expect(settingsTopicsForSearch('', t)).toHaveLength(8);
  });

  it('normalizes case and diacritics before matching', () => {
    expect(normalizeSettingsSearch('BANDEJA')).toBe('bandeja');
    expect(normalizeSettingsSearch('Inicialização')).toBe('inicializacao');
    expect(normalizeSettingsSearch('  Configurações  ')).toBe('configuracoes');
  });

  it('finds Geral by the tray row labels and filters the navigation groups', () => {
    expect(settingsTopicsForSearch('bandeja', t).map((topic) => topic.id)).toEqual([
      'general',
    ]);
    expect(settingsTopicsForSearch('BANDEJA', t).map((topic) => topic.id)).toEqual([
      'general',
    ]);
    expect(
      settingsTopicsForSearch('inicializacao', t).map((topic) => topic.id),
    ).toEqual(['general']);

    const groups = settingsNavGroupsForSearch('bandeja', t);
    expect(groups).toHaveLength(1);
    expect(groups[0].id).toBe('app');
    expect(groups[0].topics.map((topic) => topic.id)).toEqual(['general']);
  });

  it('returns no topic for text that matches nothing', () => {
    expect(settingsTopicsForSearch('zzzz-nao-existe', t)).toEqual([]);
    expect(settingsNavGroupsForSearch('zzzz-nao-existe', t)).toEqual([]);
  });
});
