import React from 'react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { I18nProvider } from './i18n';

export function createAppQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: Infinity,
        retry: false,
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
    <QueryClientProvider client={client}>
      <I18nProvider initialLocale={initialLocale}>{children}</I18nProvider>
    </QueryClientProvider>
  );
};
