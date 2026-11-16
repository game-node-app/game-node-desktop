import { queryOptions } from '@tanstack/react-query';
import { commands } from './commands';

export const SNAPSHOT_QUERY_KEY = ['snapshot'] as const;
export const APP_INFO_QUERY_KEY = ['app-info'] as const;
export const LICENSE_NOTICES_QUERY_KEY = ['license-notices'] as const;
export const SIGN_IN_COVER_WALL_QUERY_KEY = ['sign-in-cover-wall'] as const;

export const appInfoQueryOptions = queryOptions({
  queryKey: APP_INFO_QUERY_KEY,
  queryFn: () => commands.getAppInfo(),
  staleTime: Infinity,
  gcTime: Infinity,
});

export const signInCoverWallQueryOptions = queryOptions({
  queryKey: SIGN_IN_COVER_WALL_QUERY_KEY,
  queryFn: () => commands.signInCoverWall(),
  staleTime: 60_000,
  // The mural is rebuilt each time the signed-out screen mounts; do not keep the names cached
  // after a sign-in.
  gcTime: 0,
});

export const licenseNoticesQueryOptions = queryOptions({
  queryKey: LICENSE_NOTICES_QUERY_KEY,
  queryFn: () => commands.getLicenseNotices(),
  staleTime: Infinity,
  // The notices payload is ~4.8 MB. Drop it when the last viewer unmounts instead
  // of retaining it for the default 5-minute garbage-collection window.
  gcTime: 0,
});
