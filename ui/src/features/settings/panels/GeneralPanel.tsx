import React, { useEffect, useState } from 'react';
import { IconAlertTriangle, IconKeyboard } from '@tabler/icons-react';
import {
  commands,
  useAppInfo,
  useLibraryView,
  useSnapshot,
  type AppPreferencePatch,
  type AppPreferences,
  type CoverSize,
  type LocalePreference,
  type StartScopePreference,
  type ViewMode,
} from '../../../api';
import {
  Button,
  Kbd,
  SegmentedControl,
  SelectField,
  Switch,
} from '../../../components/ui';
import { openShortcuts } from '../../../store/uiStore';
import { useI18n } from '../../../i18n';
import { SettingsCard, SettingsPanel, SettingsRow } from '../components';

const START_SCOPES: StartScopePreference[] = [
  'last',
  'all',
  'installed',
  'favorites',
  'recent',
];

const COVER_SIZES: CoverSize[] = ['small', 'medium', 'large'];

const LOCALES: LocalePreference[] = ['system', 'en', 'ptBr'];

/**
 * The Geral topic (S8): PC preferences for startup (Windows only), the library
 * and the interface language, plus the shortcut to the keyboard shortcuts
 * dialog. Every control applies immediately with an optimistic value; a failed
 * command restores the saved value and shows the error in this panel (D4, D6–D8).
 */
export const GeneralPanel: React.FC = () => {
  const { t, setLocale } = useI18n();
  const { data: appInfo } = useAppInfo();
  const library = useLibraryView();
  const { data: preferences } = useSnapshot((snapshot) => snapshot.preferences);

  // Values shown while the command is in flight. Each entry is dropped as soon
  // as the published snapshot carries the same value, or on a command error.
  const [overrides, setOverrides] = useState<Partial<AppPreferences>>({});
  const [viewOverride, setViewOverride] = useState<ViewMode | null>(null);
  const [coverSizeOverride, setCoverSizeOverride] = useState<CoverSize | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);

  const layout = library?.layout;
  const savedView = layout?.view ?? 'grid';
  const savedCoverSize = layout?.coverSize ?? 'medium';
  const view = viewOverride ?? savedView;
  const coverSize = coverSizeOverride ?? savedCoverSize;

  const showStartupCard =
    appInfo?.platform === 'windows' && appInfo.backgroundSupported;

  const preferenceValue = <K extends keyof AppPreferences>(
    key: K,
  ): AppPreferences[K] =>
    Object.prototype.hasOwnProperty.call(overrides, key)
      ? (overrides[key] as AppPreferences[K])
      : preferences[key];

  // Once the host publishes a matching value the optimistic copy is redundant.
  useEffect(() => {
    setOverrides((previous) => {
      const entries = Object.entries(previous);
      if (entries.length === 0) return previous;
      const next: Partial<AppPreferences> = { ...previous };
      let changed = false;
      for (const [key, value] of entries) {
        const typedKey = key as keyof AppPreferences;
        if (preferences[typedKey] === value) {
          delete next[typedKey];
          changed = true;
        }
      }
      return changed ? next : previous;
    });
  }, [preferences]);

  useEffect(() => {
    if (viewOverride !== null && savedView === viewOverride) {
      setViewOverride(null);
    }
  }, [savedView, viewOverride]);

  useEffect(() => {
    if (coverSizeOverride !== null && savedCoverSize === coverSizeOverride) {
      setCoverSizeOverride(null);
    }
  }, [coverSizeOverride, savedCoverSize]);

  const localeTarget = (preference: LocalePreference): string | undefined => {
    switch (preference) {
      case 'system':
        return appInfo?.systemLocale ?? appInfo?.locale;
      case 'ptBr':
        return 'pt-BR';
      case 'en':
        return 'en';
      default:
        return undefined;
    }
  };

  const applyLocale = (preference: LocalePreference) => {
    const target = localeTarget(preference);
    if (target) setLocale(target);
  };

  /**
   * Persists one preference. On failure the optimistic value is dropped, so the
   * control falls back to the last snapshot, and the panel shows the error.
   */
  const applyPreference = async <K extends keyof AppPreferences>(
    key: K,
    value: AppPreferences[K],
    patch: AppPreferencePatch,
  ): Promise<boolean> => {
    setOverrides((previous) => ({ ...previous, [key]: value }));
    setError(null);
    try {
      await commands.setAppPreference(patch);
      return true;
    } catch {
      setOverrides((previous) => {
        const next = { ...previous };
        delete next[key];
        return next;
      });
      setError(t('settings.general.saveFailed'));
      return false;
    }
  };

  const changeLocale = async (next: LocalePreference) => {
    if (next === preferenceValue('locale')) return;
    const previous = preferences.locale;
    applyLocale(next);
    const saved = await applyPreference('locale', next, { locale: next });
    if (!saved) applyLocale(previous);
  };

  const changeView = async (next: ViewMode) => {
    if (next === view) return;
    setViewOverride(next);
    setError(null);
    try {
      await commands.libraryInput({ setView: next });
    } catch {
      setViewOverride(null);
      setError(t('settings.general.saveFailed'));
    }
  };

  const changeCoverSize = async (next: CoverSize) => {
    if (next === coverSize) return;
    setCoverSizeOverride(next);
    setError(null);
    try {
      await commands.libraryInput({ setCoverSize: next });
    } catch {
      setCoverSizeOverride(null);
      setError(t('settings.general.saveFailed'));
    }
  };

  const coverSizeOptions = COVER_SIZES.map((size) => ({
    value: size,
    // The visible caption is the single letter; the full name is the accessible
    // option name (`copy.md` › S8).
    label: (
      <>
        <span aria-hidden="true">{t(`settings.general.cover.${size}`)}</span>
        <span className="sr-only">
          {t(`settings.general.cover.${size}Label`)}
        </span>
      </>
    ),
  }));

  return (
    <SettingsPanel
      title={t('settings.nav.general')}
      description={t('settings.general.description')}
    >
      {error ? (
        <div
          role="alert"
          data-testid="settings-general-error"
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

      {showStartupCard ? (
        <SettingsCard title={t('settings.general.startup')}>
          <SettingsRow
            label={t('settings.general.startWithWindows')}
            help={`${t('settings.general.startWithWindowsHelp')} ${t(
              'settings.general.startWithWindowsInstalledOnly',
            )}`}
            control={(ids) => (
              <Switch
                aria-labelledby={ids.labelId}
                aria-describedby={ids.describedBy}
                checked={preferenceValue('startWithWindows')}
                onChange={(event) =>
                  void applyPreference(
                    'startWithWindows',
                    event.currentTarget.checked,
                    { startWithWindows: event.currentTarget.checked },
                  )
                }
              />
            )}
          />
          <SettingsRow
            label={t('settings.general.startMinimized')}
            control={(ids) => (
              <Switch
                aria-labelledby={ids.labelId}
                checked={preferenceValue('startMinimized')}
                onChange={(event) =>
                  void applyPreference(
                    'startMinimized',
                    event.currentTarget.checked,
                    { startMinimized: event.currentTarget.checked },
                  )
                }
              />
            )}
          />
          <SettingsRow
            label={t('settings.general.closeToTray')}
            help={
              preferenceValue('closeToTray')
                ? t('settings.general.closeToTrayHelp')
                : t('settings.general.closeToTrayHelpOff')
            }
            control={(ids) => (
              <Switch
                aria-labelledby={ids.labelId}
                aria-describedby={ids.describedBy}
                checked={preferenceValue('closeToTray')}
                onChange={(event) =>
                  void applyPreference(
                    'closeToTray',
                    event.currentTarget.checked,
                    { closeToTray: event.currentTarget.checked },
                  )
                }
              />
            )}
          />
        </SettingsCard>
      ) : null}

      <SettingsCard title={t('settings.general.library')}>
        <SettingsRow
          label={t('settings.general.defaultView')}
          control={(ids) => (
            <SegmentedControl<ViewMode>
              aria-labelledby={ids.labelId}
              segmentPadding="regular"
              data={[
                { value: 'grid', label: t('settings.general.view.grid') },
                { value: 'list', label: t('settings.general.view.list') },
              ]}
              value={view}
              onChange={(next) => void changeView(next)}
            />
          )}
        />
        <SettingsRow
          label={t('settings.general.coverSize')}
          control={(ids) => (
            <SegmentedControl<CoverSize>
              aria-labelledby={ids.labelId}
              segmentPadding="regular"
              data={coverSizeOptions}
              value={coverSize}
              onChange={(next) => void changeCoverSize(next)}
            />
          )}
        />
        <SettingsRow
          label={t('settings.general.openIn')}
          control={(ids) => (
            <SelectField
              size="compact"
              className="h-[32px] w-[180px]"
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              data-testid="settings-general-open-in"
              value={preferenceValue('startScope')}
              options={START_SCOPES.map((scope) => ({
                value: scope,
                label: t(`settings.general.scope.${scope}`),
              }))}
              onChange={(value) =>
                void applyPreference('startScope', value as StartScopePreference, {
                  startScope: value as StartScopePreference,
                })
              }
            />
          )}
        />
        <SettingsRow
          label={t('settings.general.showStatusBar')}
          control={(ids) => (
            <Switch
              aria-labelledby={ids.labelId}
              checked={preferenceValue('showStatusBar')}
              onChange={(event) =>
                void applyPreference(
                  'showStatusBar',
                  event.currentTarget.checked,
                  { showStatusBar: event.currentTarget.checked },
                )
              }
            />
          )}
        />
      </SettingsCard>

      <SettingsCard title={t('settings.general.languageCard')}>
        <SettingsRow
          label={t('settings.general.language')}
          help={t('settings.general.languageHelp')}
          control={(ids) => (
            <SelectField
              size="compact"
              className="h-[32px] w-[180px]"
              aria-labelledby={ids.labelId}
              aria-describedby={ids.describedBy}
              data-testid="settings-general-language"
              value={preferenceValue('locale')}
              options={LOCALES.map((locale) => ({
                value: locale,
                label: t(`settings.general.language.${locale}`),
              }))}
              onChange={(value) => void changeLocale(value as LocalePreference)}
            />
          )}
        />
        <SettingsRow
          label={t('settings.general.shortcuts')}
          help={t('settings.general.shortcutsHelp')}
          control={
            <>
              <Kbd aria-hidden="true">?</Kbd>
              <Button
                size="sm"
                variant="secondary"
                leftIcon={IconKeyboard}
                onClick={openShortcuts}
              >
                {t('settings.general.viewShortcuts')}
              </Button>
            </>
          }
        />
      </SettingsCard>
    </SettingsPanel>
  );
};
