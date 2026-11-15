import type { ReactElement } from 'react';
import { MantineProvider } from '@mantine/core';
import { QueryClientProvider } from '@tanstack/react-query';
import { render, type RenderResult } from '@testing-library/react';
import { vi } from 'vitest';
import { appTheme, createAppQueryClient } from '../../AppProviders';
import {
  SNAPSHOT_QUERY_KEY,
  commands,
  createDefaultLibraryLayout,
  createDefaultLibraryView,
  createDefaultSnapshot,
  type LibraryLayout,
  type LibraryView,
  type Snapshot,
} from '../../api';
import { I18nProvider, type SupportedLocale } from '../../i18n';

/** Builds a complete library view, deep-merging only the layout and actions. */
export function makeLibrary(
  overrides: Omit<Partial<LibraryView>, 'layout'> & {
    layout?: Partial<LibraryLayout>;
  } = {},
): LibraryView {
  const base = createDefaultLibraryView();
  return {
    ...base,
    ...overrides,
    layout: {
      ...createDefaultLibraryLayout(),
      ...(overrides.layout ?? {}),
    } as LibraryLayout,
    actions: {
      ...base.actions,
      ...(overrides.actions ?? {}),
    },
    filterCounts: overrides.filterCounts ?? base.filterCounts,
    scopeCounts: overrides.scopeCounts ?? base.scopeCounts,
  };
}

export function renderControls(
  ui: ReactElement,
  library: LibraryView = makeLibrary(),
  locale: SupportedLocale = 'en',
  snapshotOverrides: Partial<Snapshot> = {},
): RenderResult {
  const snapshot: Snapshot = {
    ...createDefaultSnapshot(),
    ...snapshotOverrides,
    library,
  };
  const queryClient = createAppQueryClient();
  queryClient.setQueryData(SNAPSHOT_QUERY_KEY, snapshot);

  return render(
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={queryClient}>
        <I18nProvider initialLocale={locale}>{ui}</I18nProvider>
      </QueryClientProvider>
    </MantineProvider>,
  );
}

/** Mocks the library input command used by every control in this unit. */
export function spyLibraryInput() {
  return vi.spyOn(commands, 'libraryInput').mockResolvedValue(undefined);
}

export function setInputs(spy: ReturnType<typeof spyLibraryInput>) {
  return spy.mock.calls.map(([input]) => input);
}
