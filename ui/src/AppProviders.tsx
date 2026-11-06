import React from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import {
  alpha,
  colorsTuple,
  createTheme,
  defaultVariantColorsResolver,
  MantineProvider,
  parseThemeColor,
  type VariantColorsResolver,
} from '@mantine/core';
import { I18nProvider } from './i18n';
import { SnapshotStreamProvider } from './api/useSnapshot';

/**
 * Mantine derives the dark "light" variant from shades 9 (background) and 0
 * (text). Flat palettes such as `secondary` and `tertiary` produce orange-on-brown
 * results, so tint the surface with the color itself instead.
 */
export const appVariantColorResolver: VariantColorsResolver = (input) => {
  const defaults = defaultVariantColorsResolver(input);
  if (input.variant !== 'light' || !input.color) return defaults;
  const parsed = parseThemeColor({ color: input.color, theme: input.theme });
  const shades = input.theme.colors[parsed.color];
  if (!parsed.isThemeColor || !shades || new Set(shades).size > 1) {
    return defaults;
  }
  return {
    ...defaults,
    background: alpha(shades[0], 0.15),
    hover: alpha(shades[0], 0.25),
    color: shades[0],
  };
};

/** GameNode Version 3.0 theme, aligned with `game-node-clients`. */
export const appTheme = createTheme({
  fontFamily:
    "Roboto, -apple-system, BlinkMacSystemFont, 'Segoe UI', Oxygen, Ubuntu, Cantarell, sans-serif",
  headings: {
    fontFamily:
      "Roboto, -apple-system, BlinkMacSystemFont, 'Segoe UI', Oxygen, Ubuntu, Cantarell, sans-serif",
  },
  colors: {
    brand: [
      '#ffede5',
      '#ffd9cf',
      '#fbb3a0',
      '#f7896d',
      '#f36742',
      '#f15025',
      '#f14517',
      '#d6360b',
      '#c02d06',
      '#a82301',
    ],
    secondary: colorsTuple('#1A1A1A'),
    tertiary: colorsTuple('#872C13'),
    paper: colorsTuple('#161616'),
    chrome: colorsTuple('#111111'),
  },
  primaryColor: 'brand',
  defaultRadius: 'md',
  variantColorResolver: appVariantColorResolver,
});

export function createAppQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        // The Rust engine pushes authoritative state; queries never go stale on their own.
        staleTime: Infinity,
        // Local IPC is not HTTP. navigator.onLine must not pause queries on an offline machine.
        networkMode: 'always',
        retry: false,
        // Window focus/reconnect are web concepts; Tauri fires them often and neither
        // implies the engine changed. Events are the refresh mechanism.
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      },
    },
  });
}

export interface AppProvidersProps {
  queryClient?: QueryClient;
  initialLocale?: string;
  children?: React.ReactNode;
}

/** The provider stack shared by the real entry point and the integration tests. */
export const AppProviders: React.FC<AppProvidersProps> = ({
  queryClient,
  initialLocale,
  children,
}) => {
  const [client] = React.useState(() => queryClient ?? createAppQueryClient());
  return (
    <MantineProvider theme={appTheme} defaultColorScheme="dark">
      <QueryClientProvider client={client}>
        <SnapshotStreamProvider>
          <I18nProvider initialLocale={initialLocale}>{children}</I18nProvider>
        </SnapshotStreamProvider>
      </QueryClientProvider>
    </MantineProvider>
  );
};
