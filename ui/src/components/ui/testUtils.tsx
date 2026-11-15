import type { ReactElement } from 'react';
import { MantineProvider } from '@mantine/core';
import { render, type RenderOptions, type RenderResult } from '@testing-library/react';
import { appTheme } from '../../AppProviders';
import { I18nProvider, type SupportedLocale } from '../../i18n';

export interface UiRenderResult extends Omit<RenderResult, 'rerender'> {
  rerender: (ui: ReactElement) => void;
}

/** Renders a primitive inside the same Mantine theme the shell uses. */
export function renderUi(
  ui: ReactElement,
  options?: Omit<RenderOptions, 'wrapper'>,
): UiRenderResult {
  const result = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      {ui}
    </MantineProvider>,
    options,
  );

  return {
    ...result,
    rerender: (next: ReactElement) =>
      result.rerender(
        <MantineProvider theme={appTheme} defaultColorScheme="dark">
          {next}
        </MantineProvider>,
      ),
  };
}

/**
 * Renders a primitive with both the shell theme and a locale. Primitives that
 * read copy through `useI18n` use this helper so their tests cover en and pt-BR.
 */
export function renderUiWithI18n(
  ui: ReactElement,
  locale: SupportedLocale = 'en',
  options?: Omit<RenderOptions, 'wrapper'>,
): UiRenderResult {
  const wrap = (element: ReactElement) => (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <I18nProvider initialLocale={locale}>{element}</I18nProvider>
    </MantineProvider>
  );

  const result = render(wrap(ui), options);

  return {
    ...result,
    rerender: (next: ReactElement) => result.rerender(wrap(next)),
  };
}
