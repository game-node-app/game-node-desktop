import type { ReactElement } from 'react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { act, render, type RenderResult } from '@testing-library/react';
import {
  SNAPSHOT_QUERY_KEY,
  createDefaultCatalogView,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type CatalogView as CatalogViewState,
  type LibraryView,
} from '../../api';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import { I18nProvider, type SupportedLocale } from '../../i18n';

/** A complete catalog slice with only the fields a test changes. */
export function makeCatalog(
  overrides: Partial<CatalogViewState> = {},
): CatalogViewState {
  return { ...createDefaultCatalogView(), ...overrides };
}

/** A complete library view whose layout is deep-merged like the app fixtures. */
export function makeCatalogLibrary(
  overrides: Partial<LibraryView> = {},
): LibraryView {
  const base = createDefaultLibraryView();
  return {
    ...base,
    ...overrides,
    layout: {
      ...createDefaultLibraryLayout(),
      ...(overrides.layout ?? {}),
    },
  };
}

export interface CatalogRenderResult extends RenderResult {
  /** Pushes a new library view through the same snapshot query the app uses. */
  update: (library: LibraryView) => void;
}

/** Renders the catalog with the shell providers and an updatable snapshot cache. */
export function renderCatalog(
  ui: ReactElement,
  library: LibraryView,
  locale: SupportedLocale = 'en',
): CatalogRenderResult {
  const queryClient = createAppQueryClient();
  const snapshot = createDefaultSnapshot();
  snapshot.library = library;
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

  const result = render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );

  const update = (next: LibraryView) => {
    const nextSnapshot = createDefaultSnapshot();
    nextSnapshot.library = next;
    act(() => {
      queryClient.setQueryData(SNAPSHOT_QUERY_KEY, nextSnapshot);
    });
  };

  return { ...result, update };
}
