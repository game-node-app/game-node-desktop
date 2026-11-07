import { queryOptions } from '@tanstack/react-query';
import { commands } from './commands';

export const SNAPSHOT_QUERY_KEY = ['snapshot'] as const;
export const APP_INFO_QUERY_KEY = ['app-info'] as const;
export const LICENSE_NOTICES_QUERY_KEY = ['license-notices'] as const;

export const appInfoQueryOptions = queryOptions({
  queryKey: APP_INFO_QUERY_KEY,
  queryFn: () => commands.getAppInfo(),
  staleTime: Infinity,
  gcTime: Infinity,
});

export const licenseNoticesQueryOptions = queryOptions({
  queryKey: LICENSE_NOTICES_QUERY_KEY,
  queryFn: () => commands.getLicenseNotices(),
  staleTime: Infinity,
});
